use crate::config::SkillAgent;
use crate::environment::manifest::{edit, validate_repo, Scope};
use crate::environment::review::FileStatus;
use crate::environment::{Action, Env, Plan, Status, SyncOptions, Update};
use crate::gh::{Gh, GhClient, TreeEntry};
use crate::{ui, util};
use anyhow::{bail, Result};
use clap::Subcommand;
use colored::Colorize;
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Subcommand)]
pub enum AddKind {
    /// Add a skill from a GitHub repository
    Skill {
        /// GitHub repository (owner/repo)
        repo: String,
        /// Skill name, or its directory path inside the repository
        skill: String,
        /// Branch, tag, or commit (default: latest release, else default branch)
        #[arg(long)]
        rev: Option<String>,
        /// Install under a different name
        #[arg(long)]
        name: Option<String>,
    },
}

/// Defers `gh` discovery until a command actually needs the network, so
/// `agt sync` on an up-to-date lock with a warm cache works without it.
struct LazyGh(OnceCell<Gh>);

impl LazyGh {
    fn new() -> Self {
        Self(OnceCell::new())
    }

    fn get(&self) -> Result<&Gh> {
        if self.0.get().is_none() {
            let _ = self.0.set(Gh::new()?);
        }
        Ok(self.0.get().expect("initialized above"))
    }
}

impl GhClient for LazyGh {
    fn resolve_commit(&self, repo: &str, rev: Option<&str>) -> Result<(String, String)> {
        self.get()?.resolve_commit(repo, rev)
    }

    fn tree(&self, repo: &str, tree: &str) -> Result<Vec<TreeEntry>> {
        self.get()?.tree(repo, tree)
    }

    fn install_skill(&self, repo: &str, path: &str, commit: &str, dir: &Path) -> Result<()> {
        self.get()?.install_skill(repo, path, commit, dir)
    }
}

pub fn init(global: bool, agents: Vec<SkillAgent>) -> Result<()> {
    let scope = Scope::resolve(global)?;
    let path = scope.manifest_path();
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    let agents = if agents.is_empty() {
        vec![SkillAgent::Claude]
    } else {
        agents
    };
    edit::save(&path, &edit::starter(&agents))?;
    ui::success(&format!("Created {}", path.display()));
    ui::hint(&format!(
        "Add skills with `agt add{} skill <owner/repo> <skill>`",
        scope.flag()
    ));
    Ok(())
}

pub fn add(global: bool, kind: AddKind) -> Result<()> {
    let AddKind::Skill {
        repo,
        skill,
        rev,
        name,
    } = kind;
    validate_repo(&repo)?;
    let skill = skill.trim_matches('/');
    let (path, default_name) = match skill.rsplit_once('/') {
        Some((_, leaf)) => {
            crate::remote::validate_source_path(skill)?;
            (Some(skill), leaf)
        }
        None => (None, skill),
    };
    let name = name.as_deref().unwrap_or(default_name);
    util::validate_name(name)?;

    let scope = Scope::resolve(global)?;
    let manifest_path = scope.manifest_path();
    let previous = fs::read_to_string(&manifest_path).ok();
    let mut doc = if previous.is_some() {
        edit::load(&manifest_path)?
    } else {
        ui::info(&format!("Creating {}", manifest_path.display()));
        edit::starter(&[SkillAgent::Claude])
    };
    // A bare skill name is resolved by search; keep the found path in the lock only.
    edit::add_skill(&mut doc, name, &repo, rev.as_deref(), path)?;
    edit::save(&manifest_path, &doc)?;

    let result = run_sync(&scope, &SyncOptions::default());
    if result.is_err() {
        restore(&manifest_path, previous.as_deref())?;
    }
    result
}

pub fn remove(global: bool, name: &str) -> Result<()> {
    let scope = Scope::resolve(global)?;
    let manifest_path = scope.manifest_path();
    let previous = fs::read_to_string(&manifest_path).ok();
    let mut doc = edit::load(&manifest_path)?;
    if !edit::remove_skill(&mut doc, name) {
        bail!("'{name}' is not declared in {}", manifest_path.display());
    }
    edit::save(&manifest_path, &doc)?;
    let result = run_sync(&scope, &SyncOptions::default());
    if result.is_err() {
        restore(&manifest_path, previous.as_deref())?;
    }
    result
}

pub fn adopt(global: bool, name: &str) -> Result<()> {
    util::validate_name(name)?;
    let origin = [SkillAgent::Claude, SkillAgent::Codex]
        .into_iter()
        .map(|agent| crate::config::skill_target(global, agent).join(name))
        .filter(|dir| dir.is_dir() && !dir.is_symlink())
        .find_map(|dir| {
            let skill_md = fs::read_to_string(dir.join("SKILL.md")).ok();
            let origin = crate::doctor::scan::origin(&dir, skill_md.as_deref());
            origin.github().map(|(repo, path, rev)| {
                (repo.to_string(), path.to_string(), rev.map(String::from))
            })
        });
    let Some((repo, path, rev)) = origin else {
        bail!(
            "No unmanaged '{name}' with a known GitHub origin was found. \
             Declare it with `agt add skill <owner/repo> <path> --name {name}` instead."
        );
    };

    let scope = Scope::resolve(global)?;
    let manifest_path = scope.manifest_path();
    let previous = fs::read_to_string(&manifest_path).ok();
    let mut doc = if previous.is_some() {
        edit::load(&manifest_path)?
    } else {
        ui::info(&format!("Creating {}", manifest_path.display()));
        edit::starter(&[SkillAgent::Claude])
    };
    edit::add_skill(&mut doc, name, &repo, rev.as_deref(), Some(&path))?;
    edit::save(&manifest_path, &doc)?;

    let gh = LazyGh::new();
    let result = Env::new(scope.clone(), &gh)
        .and_then(|env| env.adopt(name))
        .and_then(|adopted| {
            for (agent, backup) in adopted {
                match backup {
                    None => ui::info(&format!(
                        "{agent}: adopted in place (matches the locked commit)"
                    )),
                    Some(path) => ui::warn(&format!(
                        "{agent}: existing copy differed from {repo}@{}; moved to {}",
                        rev.as_deref().unwrap_or("default"),
                        path.display()
                    )),
                }
            }
            run_sync(&scope, &SyncOptions::default())
        });
    if result.is_err() {
        restore(&manifest_path, previous.as_deref())?;
    }
    result
}

pub fn sync(global: bool, check: bool, frozen: bool) -> Result<()> {
    let scope = Scope::resolve(global)?;
    let opts = SyncOptions {
        check,
        frozen,
        ..Default::default()
    };
    run_sync(&scope, &opts)
}

pub fn lock(global: bool, update: Option<Vec<String>>, check: bool) -> Result<()> {
    let gh = LazyGh::new();
    let env = Env::new(Scope::resolve(global)?, &gh)?;
    let manifest = env.load_manifest()?;
    let deps = manifest.deps()?;
    let mut lock = env.load_lock()?;
    if check {
        let stale = crate::environment::stale_entries(&deps, &lock, &BTreeSet::new());
        if stale.is_empty() && env.scope.lock_path().exists() {
            ui::success("agt.lock matches agt.toml");
            return Ok(());
        }
        bail!(
            "agt.lock is out of date{}. Run `agt lock{}` and commit the result.",
            if stale.is_empty() {
                String::new()
            } else {
                format!(" for: {}", stale.join(", "))
            },
            env.scope.flag()
        );
    }
    let refresh: BTreeSet<String> = match update {
        Some(names) if names.is_empty() => deps.iter().map(|d| d.name.clone()).collect(),
        Some(names) => {
            for name in &names {
                if !deps.iter().any(|d| &d.name == name) {
                    bail!("'{name}' is not declared in agt.toml");
                }
            }
            names.into_iter().collect()
        }
        None => BTreeSet::new(),
    };
    let changed = env.relock(&deps, &mut lock, &refresh)?;
    let lock_path = env.scope.lock_path();
    if changed.is_empty() && lock_path.exists() {
        ui::success("agt.lock is up to date");
        return Ok(());
    }
    lock.save(&lock_path)?;
    ui::success(&format!(
        "Locked {} package(s) in {}",
        lock.packages.len(),
        lock_path.display()
    ));
    ui::hint(&format!("Run `agt sync{}` to install", env.scope.flag()));
    Ok(())
}

pub fn outdated(global: bool, json: bool) -> Result<()> {
    let gh = LazyGh::new();
    let env = Env::new(Scope::resolve(global)?, &gh)?;
    let rows = env.outdated()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    let mut pending = 0;
    for row in &rows {
        let locked = row
            .locked
            .as_deref()
            .map(short)
            .unwrap_or_else(|| "-".into());
        let (mark, detail) = match &row.status {
            Status::Current => ("=".dimmed(), "up to date".dimmed().to_string()),
            Status::CommitOnly { rev, commit } => (
                "=".dimmed(),
                format!("{rev} moved to {} (skill unchanged)", short(commit))
                    .dimmed()
                    .to_string(),
            ),
            Status::Changed { rev, commit } => {
                pending += 1;
                ("↑".green(), format!("{locked} → {} ({rev})", short(commit)))
            }
            Status::Missing { rev, commit } => {
                pending += 1;
                (
                    "!".red(),
                    format!("removed upstream at {rev} ({})", short(commit)),
                )
            }
            Status::Pinned => ("·".dimmed(), "pinned to a commit".dimmed().to_string()),
            Status::NotLocked => {
                pending += 1;
                ("?".yellow(), "not locked yet; run `agt lock`".to_string())
            }
        };
        println!("  {mark} {:<24} {detail}", row.name);
    }
    if pending > 0 {
        ui::hint(&format!(
            "Run `agt update{}` to review and apply",
            env.scope.flag()
        ));
    } else {
        ui::success("Everything is up to date");
    }
    Ok(())
}

pub fn update(global: bool, names: Vec<String>, yes: bool, yes_all: bool) -> Result<()> {
    let gh = LazyGh::new();
    let env = Env::new(Scope::resolve(global)?, &gh)?;
    let names: BTreeSet<String> = names.into_iter().collect();
    let updates = env.plan_updates(&names)?;
    if updates.is_empty() {
        ui::success("Everything is up to date");
        return Ok(());
    }

    let interactive = !yes && console::Term::stderr().is_term();
    let mut approved = Vec::new();
    let mut held = Vec::new();
    for update in updates {
        print_update(&update);
        let needs_approval = update.review.as_ref().is_some_and(|r| r.needs_approval());
        let apply = if !needs_approval || yes_all {
            true
        } else if interactive {
            ask_apply(&update)?
        } else {
            false
        };
        if apply {
            approved.push(update.new);
        } else {
            held.push(update.new.name);
        }
    }

    if !approved.is_empty() {
        let plan = env.apply_updates(approved)?;
        print_plan(&plan, false);
    }
    if !held.is_empty() {
        bail!(
            "Not applied: {}. Executable content changed; review it interactively or pass --yes-all.",
            held.join(", ")
        );
    }
    Ok(())
}

fn print_update(update: &Update) {
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    println!(
        "\n{} {} → {} ({})",
        update.new.name.bold(),
        short(&update.old.commit),
        short(&update.new.commit),
        update.new.rev
    );
    let Some(review) = &update.review else {
        println!("    {}", "skill content unchanged".dimmed());
        return;
    };
    for file in &review.files {
        let status = match file.status {
            FileStatus::Added => "added".green(),
            FileStatus::Removed => "removed".red(),
            FileStatus::Modified => "modified".yellow(),
            FileStatus::Mode => "mode".yellow(),
        };
        let exec = if file.executable {
            format!("  {}", "⚠ executable".yellow())
        } else {
            String::new()
        };
        println!(
            "    {:<8} {:<40} {}{exec}",
            status,
            file.path,
            format!("+{} −{}", file.added, file.removed).dimmed()
        );
    }
    if let Some((before, after)) = &review.allowed_tools {
        println!(
            "    {} allowed-tools: {} → {}",
            "⚠".yellow(),
            before.as_deref().unwrap_or("(none)"),
            after.as_deref().unwrap_or("(none)")
        );
    }
    for risk in &review.risks {
        println!(
            "    {} {} ({}): {}",
            "⚠".red(),
            risk.path,
            risk.reason,
            risk.line.dimmed()
        );
    }
}

fn ask_apply(update: &Update) -> Result<bool> {
    let options = ["Apply", "Skip", "Show full diff"];
    loop {
        let choice = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Update {}?", update.new.name))
            .items(options)
            .default(1)
            .interact()?;
        match choice {
            0 => return Ok(true),
            1 => return Ok(false),
            _ => {
                if let Some(review) = &update.review {
                    for file in &review.files {
                        print!("{}", file.diff);
                    }
                }
            }
        }
    }
}

fn run_sync(scope: &Scope, opts: &SyncOptions) -> Result<()> {
    let gh = LazyGh::new();
    let env = Env::new(scope.clone(), &gh)?;
    let plan = env.sync(opts)?;
    print_plan(&plan, opts.check);
    if opts.check && !plan.is_empty() {
        bail!("Environment does not match agt.lock");
    }
    Ok(())
}

fn print_plan(plan: &Plan, check: bool) {
    if plan.is_empty() {
        ui::success("Environment is up to date");
        return;
    }
    if !plan.stale.is_empty() {
        let verb = if check {
            "lock out of date"
        } else {
            "lock updated"
        };
        ui::info(&format!("{verb}: {}", plan.stale.join(", ")));
    }
    for op in &plan.ops {
        let (sign, label) = match op.action {
            Action::Install => ("+".green(), "install"),
            Action::Replace => ("~".yellow(), "replace"),
            Action::Remove => ("-".red(), "remove"),
        };
        let label = if check {
            format!("would {label}")
        } else {
            label.to_string()
        };
        println!(
            "  {sign} {:<24} {:<6} {}",
            op.name,
            op.agent,
            label.dimmed()
        );
    }
    if !check {
        ui::success(&format!("Applied {} change(s)", plan.ops.len()));
    }
}

fn restore(path: &Path, previous: Option<&str>) -> Result<()> {
    match previous {
        Some(content) => fs::write(path, content)?,
        None => fs::remove_file(path)?,
    }
    ui::warn(&format!("Reverted {}", path.display()));
    Ok(())
}

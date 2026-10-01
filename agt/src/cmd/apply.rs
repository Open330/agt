use crate::config::{self, DesiredSkill, InstallMode, InstallState, SkillAgent, SkillRecord};
use crate::ui;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

const EXCLUDE_BEGIN: &str = "# >>> agt apply >>>";
const EXCLUDE_END: &str = "# <<< agt apply <<<";

#[derive(Debug, PartialEq)]
enum Action {
    /// Create the symlink
    Link(DesiredSkill),
    /// Managed symlink points at the wrong source; replace it
    Relink(DesiredSkill),
    /// Correct symlink already there but not recorded; record it
    Adopt(DesiredSkill),
    /// Managed skill the stack no longer wants
    Prune(String),
    /// Something agt does not manage occupies the name; leave it
    Conflict(DesiredSkill),
}

impl Action {
    fn changes(&self) -> bool {
        !matches!(self, Action::Conflict(_))
    }
}

/// Decide what `apply` does in one skills directory.
fn plan_dir(skills_dir: &Path, desired: &[DesiredSkill], state: &InstallState) -> Vec<Action> {
    let mut actions = Vec::new();
    for skill in desired {
        let dest = config::skill_destination(skills_dir, &skill.name);
        let managed = state.skills.get(&skill.name).is_some_and(|r| r.applied);
        if !(dest.exists() || dest.is_symlink()) {
            actions.push(Action::Link(skill.clone()));
            continue;
        }
        let points_here = fs::read_link(&dest).is_ok_and(|t| t == skill.skill_path());
        actions.push(match (points_here, managed) {
            (true, true) => {
                let recorded = &state.skills[&skill.name];
                if recorded.layer == skill.layer {
                    continue;
                }
                Action::Adopt(skill.clone())
            }
            (true, false) => Action::Adopt(skill.clone()),
            (false, true) if dest.is_symlink() => Action::Relink(skill.clone()),
            _ => Action::Conflict(skill.clone()),
        });
    }
    for (name, record) in &state.skills {
        if record.applied && !desired.iter().any(|d| &d.name == name) {
            actions.push(Action::Prune(name.clone()));
        }
    }
    actions
}

fn applied_record(skill: &DesiredSkill) -> SkillRecord {
    SkillRecord {
        layer: skill.layer.clone(),
        source: skill.source_dir.display().to_string(),
        origin: format!("{}/{}", skill.group, skill.dir),
        mode: InstallMode::Symlink,
        applied: true,
    }
}

fn execute_actions(skills_dir: &Path, actions: &[Action], state: &mut InstallState) -> Result<()> {
    fs::create_dir_all(skills_dir)?;
    for action in actions {
        match action {
            Action::Link(skill) | Action::Relink(skill) => {
                let dest = config::skill_destination(skills_dir, &skill.name);
                if matches!(action, Action::Relink(_)) {
                    fs::remove_file(&dest)
                        .with_context(|| format!("Failed to remove {}", dest.display()))?;
                }
                symlink(skill.skill_path(), &dest)
                    .with_context(|| format!("Failed to link {}", dest.display()))?;
                state.record(&skill.name, applied_record(skill));
            }
            Action::Adopt(skill) => state.record(&skill.name, applied_record(skill)),
            Action::Prune(name) => {
                let dest = config::skill_destination(skills_dir, name);
                if dest.is_symlink() {
                    fs::remove_file(&dest)
                        .with_context(|| format!("Failed to remove {}", dest.display()))?;
                } else if dest.exists() {
                    ui::warn(&format!(
                        "Not pruning {}: no longer a symlink agt created",
                        dest.display()
                    ));
                    continue;
                }
                state.forget(name);
            }
            Action::Conflict(_) => {}
        }
    }
    Ok(())
}

fn print_actions(skills_dir: &Path, actions: &[Action], dry_run: bool) {
    let changes = actions.iter().filter(|a| a.changes()).count();
    let verb = if dry_run { "would change" } else { "changed" };
    eprintln!(
        "{} {} ({} {})",
        "→".bold(),
        skills_dir.display(),
        changes,
        verb
    );
    for action in actions {
        let line = match action {
            Action::Link(s) => {
                format!("  {} {:<28} {}", "+ link".green(), s.name, s.layer.dimmed())
            }
            Action::Relink(s) => format!(
                "  {} {:<28} {}",
                "~ relink".cyan(),
                s.name,
                s.layer.dimmed()
            ),
            Action::Adopt(s) => {
                format!("  {} {:<28} {}", "= adopt".cyan(), s.name, s.layer.dimmed())
            }
            Action::Prune(n) => format!("  {} {}", "- prune".red(), n),
            Action::Conflict(s) => format!(
                "  {} {:<28} {}",
                "! skip".yellow(),
                s.name,
                "occupied by a skill agt does not manage".dimmed()
            ),
        };
        eprintln!("{line}");
    }
}

fn agent_skills_subdir(agent: SkillAgent) -> &'static str {
    match agent {
        SkillAgent::Claude => ".claude/skills",
        SkillAgent::Codex => ".agents/skills",
    }
}

/// Skills directories a target covers. Claude Code reads project skills only
/// from the directory a session starts in, not from parent directories, so a
/// directory target also gets one copy per git repository directly under it.
fn target_skill_dirs(target: &config::TargetDef) -> Result<Vec<(PathBuf, Option<PathBuf>)>> {
    if target.path == "global" {
        // Several Claude config dirs may share one skills dir through a symlink;
        // resolve it so they also share one state file.
        let dir = config::skill_target(true, target.agent);
        let dir = fs::canonicalize(&dir).unwrap_or(dir);
        return Ok(vec![(dir, None)]);
    }
    let root = config::resolve_home(&target.path);
    if !root.is_dir() {
        bail!("Target directory {} does not exist", root.display());
    }
    let sub = agent_skills_subdir(target.agent);
    let mut dirs = vec![(root.join(sub), None)];
    let mut repos: Vec<PathBuf> = fs::read_dir(&root)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.is_symlink() && p.join(".git").exists())
        .collect();
    repos.sort();
    for repo in repos {
        dirs.push((repo.join(sub), Some(repo)));
    }
    Ok(dirs)
}

/// Keep the links `apply` made out of `git status` via `.git/info/exclude`.
fn update_git_exclude(repo: &Path, skills_dir: &Path, state: &InstallState) -> Result<()> {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() {
        return Ok(()); // worktree or submodule: .git is a file; leave it alone
    }
    let rel = skills_dir.strip_prefix(repo).unwrap_or(skills_dir);
    let mut block = vec![EXCLUDE_BEGIN.to_string()];
    if let Some(state_rel) = config::state_path(rel).to_str() {
        block.push(format!("/{state_rel}"));
    }
    for (name, record) in &state.skills {
        if record.applied {
            block.push(format!("/{}/{}", rel.display(), name));
        }
    }
    block.push(EXCLUDE_END.to_string());

    let path = git_dir.join("info/exclude");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let updated = replace_block(&existing, &block.join("\n"));
    if updated != existing {
        fs::create_dir_all(git_dir.join("info"))?;
        fs::write(&path, updated).with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(())
}

fn replace_block(existing: &str, block: &str) -> String {
    let mut out = Vec::new();
    let mut inside = false;
    for line in existing.lines() {
        if line == EXCLUDE_BEGIN {
            inside = true;
            continue;
        }
        if line == EXCLUDE_END {
            inside = false;
            continue;
        }
        if !inside {
            out.push(line);
        }
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    let mut text = out.join("\n");
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(block);
    text.push('\n');
    text
}

pub fn execute(only: Option<&str>, dry_run: bool, check: bool) -> Result<()> {
    let path = config::layers_config_path();
    let cfg = config::LayersConfig::load(&path)?;
    let targets: Vec<&config::TargetDef> = cfg
        .target
        .iter()
        .filter(|t| {
            only.is_none_or(|o| {
                t.path == o || config::resolve_home(&t.path) == config::resolve_home(o)
            })
        })
        .collect();
    if targets.is_empty() {
        bail!(
            "No matching [[target]] in {}{}",
            path.display(),
            only.map(|o| format!(" for '{o}'")).unwrap_or_default()
        );
    }

    let dry_run = dry_run || check;
    let mut pending = 0;
    for target in targets {
        eprintln!(
            "{} {} ← stack {}",
            "Target".bold(),
            target.path,
            target.stack.bold()
        );
        let desired = cfg.desired_skills(&target.stack)?;
        for (skills_dir, repo) in target_skill_dirs(target)? {
            let mut state = InstallState::load(&skills_dir)?;
            let actions = plan_dir(&skills_dir, &desired, &state);
            pending += actions.iter().filter(|a| a.changes()).count();
            print_actions(&skills_dir, &actions, dry_run);
            let stack_changed = state.stack.as_deref() != Some(target.stack.as_str());
            if dry_run || (!stack_changed && !actions.iter().any(Action::changes)) {
                continue;
            }
            execute_actions(&skills_dir, &actions, &mut state)?;
            state.stack = Some(target.stack.clone());
            state.save(&skills_dir)?;
            if let Some(repo) = repo {
                update_git_exclude(&repo, &skills_dir, &state)?;
            }
        }

        if target.path == "global" && !dry_run {
            for source in cfg.static_sources(&target.stack)? {
                let source_dir = cfg.source_dir(&source)?;
                if let Err(e) = super::skill::run_manifest_setup(&source_dir) {
                    ui::warn(&format!("Static files from '{source}': {e}"));
                }
            }
        }
    }

    if check && pending > 0 {
        bail!("{pending} changes pending; run `agt apply` to converge");
    }
    if dry_run {
        ui::info(&format!("{pending} changes planned (dry run)"));
    } else {
        ui::success(&format!("{pending} changes applied"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(dir: &Path, name: &str, layer: &str) -> DesiredSkill {
        let source_dir = dir.join("src");
        let path = source_dir.join("g").join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
        DesiredSkill {
            name: name.to_string(),
            dir: name.to_string(),
            layer: layer.to_string(),
            source_dir,
            group: "g".to_string(),
        }
    }

    fn kinds(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .map(|a| match a {
                Action::Link(s) => format!("link {}", s.name),
                Action::Relink(s) => format!("relink {}", s.name),
                Action::Adopt(s) => format!("adopt {}", s.name),
                Action::Prune(n) => format!("prune {n}"),
                Action::Conflict(s) => format!("conflict {}", s.name),
            })
            .collect()
    }

    #[test]
    fn apply_links_adopts_prunes_and_leaves_unmanaged_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let wanted = skill(tmp.path(), "wanted", "p:core");
        let adopted = skill(tmp.path(), "adopted", "p:core");
        let taken = skill(tmp.path(), "taken", "p:core");
        fs::create_dir_all(&skills_dir).unwrap();
        symlink(adopted.skill_path(), skills_dir.join("adopted")).unwrap();
        fs::create_dir_all(skills_dir.join("taken")).unwrap();
        symlink(tmp.path().join("src/g/wanted"), skills_dir.join("old")).unwrap();

        let mut state = InstallState::default();
        let mut old = applied_record(&wanted);
        old.origin = "g/old".into();
        state.record("old", old);
        let mut manual = applied_record(&wanted);
        manual.applied = false;
        state.record("manual-one", manual);

        let desired = vec![wanted, adopted, taken];
        let actions = plan_dir(&skills_dir, &desired, &state);
        assert_eq!(
            kinds(&actions),
            [
                "link wanted",
                "adopt adopted",
                "conflict taken",
                "prune old"
            ]
        );

        execute_actions(&skills_dir, &actions, &mut state).unwrap();
        assert!(skills_dir.join("wanted/SKILL.md").exists());
        assert!(!skills_dir.join("old").exists());
        assert!(skills_dir.join("taken").is_dir());
        assert!(state.skills["adopted"].applied);
        assert!(state.skills.contains_key("manual-one"));
        assert!(plan_dir(&skills_dir, &desired, &state)
            .iter()
            .all(|a| !a.changes()));
    }

    #[test]
    fn exclude_block_is_replaced_not_duplicated() {
        let once = replace_block("*.log\n", "# >>> agt apply >>>\n/a\n# <<< agt apply <<<");
        let twice = replace_block(&once, "# >>> agt apply >>>\n/b\n# <<< agt apply <<<");
        assert_eq!(
            twice,
            "*.log\n\n# >>> agt apply >>>\n/b\n# <<< agt apply <<<\n"
        );
    }
}

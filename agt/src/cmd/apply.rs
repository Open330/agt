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
    /// An unrecorded symlink under another name points at this skill; move it
    /// to the install name instead of loading the skill twice
    Rename(String, DesiredSkill),
    /// Managed skill the stack no longer wants
    Prune(String),
    /// Something agt does not manage occupies the name; leave it
    Conflict(DesiredSkill),
    /// `agt skill install` already put this exact skill here; apply leaves it
    /// to that command and never prunes it
    Manual(DesiredSkill),
}

impl Action {
    fn changes(&self) -> bool {
        !matches!(self, Action::Conflict(_) | Action::Manual(_))
    }
}

/// Decide what `apply` does in one skills directory.
fn plan_dir(skills_dir: &Path, desired: &[DesiredSkill], state: &InstallState) -> Vec<Action> {
    let mut actions = Vec::new();
    for skill in desired {
        let dest = config::skill_destination(skills_dir, &skill.name);
        let recorded = state.skills.get(&skill.name);
        if !(dest.exists() || dest.is_symlink()) {
            actions.push(
                match unrecorded_link_to(skills_dir, &skill.skill_path(), state) {
                    Some(old_name) => Action::Rename(old_name, skill.clone()),
                    None => Action::Link(skill.clone()),
                },
            );
            continue;
        }
        let points_here = fs::read_link(&dest).is_ok_and(|t| t == skill.skill_path());
        actions.push(match (points_here, recorded) {
            (true, Some(r)) if r.applied && r.layer == skill.layer => continue,
            (true, Some(r)) if r.applied => Action::Adopt(skill.clone()),
            (true, Some(_)) => Action::Manual(skill.clone()),
            (true, None) => Action::Adopt(skill.clone()),
            (false, Some(r)) if r.applied && dest.is_symlink() => Action::Relink(skill.clone()),
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

/// Name of a symlink in `skills_dir` that points at `target` and that agt has
/// no record of, e.g. a link a user made by hand under the directory name.
fn unrecorded_link_to(skills_dir: &Path, target: &Path, state: &InstallState) -> Option<String> {
    let mut names: Vec<String> = fs::read_dir(skills_dir)
        .ok()?
        .flatten()
        .filter(|e| fs::read_link(e.path()).is_ok_and(|t| t == target))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| !state.skills.contains_key(name))
        .collect();
    names.sort();
    names.into_iter().next()
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
            Action::Rename(old_name, skill) => {
                let from = config::skill_destination(skills_dir, old_name);
                let dest = config::skill_destination(skills_dir, &skill.name);
                fs::rename(&from, &dest).with_context(|| {
                    format!("Failed to move {} -> {}", from.display(), dest.display())
                })?;
                state.record(&skill.name, applied_record(skill));
            }
            Action::Prune(name) => {
                let dest = config::skill_destination(skills_dir, name);
                if dest.is_symlink() {
                    fs::remove_file(&dest)
                        .with_context(|| format!("Failed to remove {}", dest.display()))?;
                } else if dest.exists() {
                    ui::warn(&format!(
                        "Not pruning {}: no longer a symlink agt created; leaving it unmanaged",
                        dest.display()
                    ));
                }
                state.forget(name);
            }
            Action::Conflict(skill) => {
                // A managed entry the user replaced with something else is theirs now.
                let dest = config::skill_destination(skills_dir, &skill.name);
                if state.skills.get(&skill.name).is_some_and(|r| r.applied) && !dest.is_symlink() {
                    state.forget(&skill.name);
                }
            }
            Action::Manual(_) => {}
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
            Action::Rename(old_name, s) => format!(
                "  {} {:<28} {}",
                "> rename".cyan(),
                s.name,
                format!("from {old_name}, {}", s.layer).dimmed()
            ),
            Action::Prune(n) => format!("  {} {}", "- prune".red(), n),
            Action::Manual(s) => format!(
                "  {} {:<28} {}",
                "= manual".dimmed(),
                s.name,
                "installed with `agt skill install`; left as is".dimmed()
            ),
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
    // The target itself may be a repository (a single app) rather than a
    // directory of repositories; either way its links stay out of git.
    let root_repo = root.join(".git").exists().then(|| root.clone());
    let mut dirs = vec![(root.join(sub), root_repo)];
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
/// The exclude file git actually reads for `repo`. For a worktree or
/// submodule `.git` is a file and the file lives in the common git dir.
fn git_exclude_path(repo: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    Some(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}

fn update_git_exclude(repo: &Path, skills_dir: &Path, state: &InstallState) -> Result<()> {
    let Some(path) = git_exclude_path(repo) else {
        ui::warn(&format!(
            "Not a git repository, links not excluded: {}",
            repo.display()
        ));
        return Ok(());
    };
    let rel = skills_dir.strip_prefix(repo).unwrap_or(skills_dir);
    let mut block = vec![EXCLUDE_BEGIN.to_string()];
    if let Some(parent) = rel.parent() {
        block.push(format!("/{}", parent.join("agt-state.json").display()));
    }
    // Only links apply still owns; a directory the user put in its place must
    // stay visible to git.
    for (name, record) in &state.skills {
        if record.applied && skills_dir.join(name).is_symlink() {
            block.push(format!("/{}/{}", rel.display(), name));
        }
    }
    block.push(EXCLUDE_END.to_string());

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let updated = replace_block(&existing, &block.join("\n"));
    if updated != existing {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
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

/// Resolve symlinks in the longest existing prefix so that a skills dir that
/// does not exist yet still compares equal to the same dir reached another way.
fn normalize(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
    let mut out = fs::canonicalize(&existing).unwrap_or(existing);
    for part in rest.into_iter().rev() {
        out.push(part);
    }
    out
}

/// Two targets writing one skills dir would prune each other's skills on
/// every run; refuse instead.
fn check_no_overlap(targets: &[config::TargetDef]) -> Result<()> {
    let mut owners: std::collections::HashMap<PathBuf, &str> = std::collections::HashMap::new();
    for target in targets {
        for (dir, _) in target_skill_dirs(target).unwrap_or_default() {
            if let Some(other) = owners.insert(normalize(&dir), &target.path) {
                if other != target.path {
                    bail!(
                        "Targets '{}' and '{}' both cover {}; give each skills directory one target",
                        other,
                        target.path,
                        dir.display()
                    );
                }
            }
        }
    }
    Ok(())
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

    check_no_overlap(&cfg.target)?;

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
            if dry_run {
                continue;
            }
            if stack_changed || actions.iter().any(Action::changes) {
                execute_actions(&skills_dir, &actions, &mut state)?;
                state.stack = Some(target.stack.clone());
                state.save(&skills_dir)?;
            }
            // Idempotent, so it also repairs repos linked before they were excluded.
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
                Action::Rename(old, s) => format!("rename {old} -> {}", s.name),
                Action::Prune(n) => format!("prune {n}"),
                Action::Conflict(s) => format!("conflict {}", s.name),
                Action::Manual(s) => format!("manual {}", s.name),
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
    fn unrecorded_link_under_another_name_is_renamed() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let mut wanted = skill(tmp.path(), "vault", "p:skills");
        wanted.name = "managing-vault".to_string();
        fs::create_dir_all(&skills_dir).unwrap();
        symlink(wanted.skill_path(), skills_dir.join("vault")).unwrap();
        let mut state = InstallState::default();

        let actions = plan_dir(&skills_dir, std::slice::from_ref(&wanted), &state);
        assert_eq!(kinds(&actions), ["rename vault -> managing-vault"]);

        execute_actions(&skills_dir, &actions, &mut state).unwrap();
        assert!(skills_dir.join("managing-vault/SKILL.md").exists());
        assert!(!skills_dir.join("vault").is_symlink());
        assert!(state.skills["managing-vault"].applied);
    }

    #[test]
    fn target_that_is_a_repo_excludes_its_own_links() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        fs::create_dir_all(app.join(".git")).unwrap();
        fs::create_dir_all(app.join("module/.git")).unwrap();
        let target = config::TargetDef {
            path: app.display().to_string(),
            stack: "ios".into(),
            agent: SkillAgent::Claude,
        };
        let dirs = target_skill_dirs(&target).unwrap();
        assert_eq!(
            dirs,
            vec![
                (app.join(".claude/skills"), Some(app.clone())),
                (app.join("module/.claude/skills"), Some(app.join("module"))),
            ]
        );
    }

    #[test]
    fn manual_install_is_neither_adopted_nor_pruned() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let manual = skill(tmp.path(), "manual-one", "p:core");
        fs::create_dir_all(&skills_dir).unwrap();
        symlink(manual.skill_path(), skills_dir.join("manual-one")).unwrap();
        let mut state = InstallState::default();
        let mut record = applied_record(&manual);
        record.applied = false;
        record.layer = "manual".into();
        state.record("manual-one", record);

        let actions = plan_dir(&skills_dir, std::slice::from_ref(&manual), &state);
        assert_eq!(kinds(&actions), ["manual manual-one"]);
        let actions = plan_dir(&skills_dir, &[], &state);
        assert!(actions.is_empty(), "{:?}", kinds(&actions));
    }

    #[test]
    fn replaced_entry_is_released_and_not_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&repo)
            .status()
            .unwrap();
        let skills_dir = repo.join(".claude/skills");
        let wanted = skill(tmp.path(), "a", "p:core");
        let mut state = InstallState::default();
        state.record("a", applied_record(&wanted));
        fs::create_dir_all(skills_dir.join("a")).unwrap(); // user replaced the link

        let actions = plan_dir(&skills_dir, std::slice::from_ref(&wanted), &state);
        assert_eq!(kinds(&actions), ["conflict a"]);
        execute_actions(&skills_dir, &actions, &mut state).unwrap();
        assert!(!state.skills.contains_key("a"));

        update_git_exclude(&repo, &skills_dir, &state).unwrap();
        let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert!(!exclude.contains("/.claude/skills/a"), "{exclude}");
    }

    #[test]
    fn overlapping_targets_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        fs::create_dir_all(work.join("app/.git")).unwrap();
        let t = |path: &Path, stack: &str| config::TargetDef {
            path: path.display().to_string(),
            stack: stack.into(),
            agent: SkillAgent::Claude,
        };
        let err = check_no_overlap(&[t(&work, "a"), t(&work.join("app"), "b")])
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("both cover"), "{err}");
        assert!(check_no_overlap(&[t(&work, "a")]).is_ok());
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

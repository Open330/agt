use crate::config;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `command` only when the Claude Code session is inside `dir`.
///
/// Claude Code reads hooks from the user settings and from the session's own
/// project directory, never from parent directories, so a hook meant for one
/// directory tree has to be installed globally and filter itself:
///
///   "command": "agt gate ~/work -- ~/work/agents/scripts/digest.sh"
///
/// Outside `dir` it exits 0 without output, which Claude Code treats as a
/// no-op for every hook event.
pub fn execute(dir: &str, command: &[String]) -> Result<()> {
    let Some((program, args)) = command.split_first() else {
        bail!("Usage: agt gate <dir> -- <command> [args...]");
    };
    // An empty dir (an unset variable in a hook) would match every session.
    let dir_path = config::resolve_home(dir.trim());
    if dir.trim().is_empty() || !dir_path.is_absolute() {
        bail!("agt gate needs an absolute directory, got '{dir}'");
    }
    let session_dir = std::env::var_os("CLAUDE_PROJECT_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .context("Cannot determine the session directory")?;
    if !is_within(&session_dir, &dir_path) {
        return Ok(());
    }
    let status = Command::new(config::resolve_home(program))
        .args(args)
        .status()
        .with_context(|| format!("Failed to run {program}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

fn is_within(path: &Path, dir: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(path).starts_with(canon(dir))
}

#[cfg(test)]
mod tests {
    use super::is_within;

    #[test]
    fn empty_or_relative_dir_is_refused() {
        for dir in ["", "  ", "work"] {
            let err = super::execute(dir, &["true".to_string()]).err().unwrap();
            assert!(
                err.to_string().contains("absolute directory"),
                "{dir:?}: {err}"
            );
        }
    }

    #[test]
    fn within_checks_whole_components() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        let other = tmp.path().join("workshop");
        std::fs::create_dir_all(work.join("repo")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        assert!(is_within(&work.join("repo"), &work));
        assert!(is_within(&work, &work));
        assert!(!is_within(&other, &work));
    }
}

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::resolve_home;

static CLAUDE_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Set by the global `--claude-dir` flag before any command runs.
pub fn set_claude_dir_override(dir: &str) {
    let _ = CLAUDE_DIR_OVERRIDE.set(resolve_home(dir));
}

/// Claude Code's user config directory: `--claude-dir`, then
/// `$CLAUDE_CONFIG_DIR`, then `~/.claude`. Skills, hooks, teams and
/// `settings.json` all live under it.
pub fn claude_config_dir() -> PathBuf {
    resolve_claude_config_dir(
        CLAUDE_DIR_OVERRIDE.get().map(PathBuf::as_path),
        std::env::var_os("CLAUDE_CONFIG_DIR"),
        dirs::home_dir(),
    )
}

fn resolve_claude_config_dir(
    flag: Option<&Path>,
    env: Option<OsString>,
    home: Option<PathBuf>,
) -> PathBuf {
    if let Some(dir) = flag {
        return dir.to_path_buf();
    }
    if let Some(dir) = env.filter(|v| !v.is_empty()) {
        return resolve_home(&dir.to_string_lossy());
    }
    home.unwrap_or_else(|| PathBuf::from("~")).join(".claude")
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    clap::ValueEnum,
    serde::Deserialize,
    serde::Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SkillAgent {
    #[default]
    Claude,
    Codex,
}

impl fmt::Display for SkillAgent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Codex => write!(f, "codex"),
        }
    }
}

const EXCLUDE_DIRS: &[&str] = &[
    "static",
    "cli",
    "codex-support",
    "personas",
    "agt",
    "npm",
    ".git",
    ".github",
    ".agents",
    ".context",
    "node_modules",
    "__pycache__",
    "hooks",
    "teams",
    "target",
];

/// Find agt source directory.
/// Priority: env var (cheapest) > walk up from exe > home dir fallbacks
pub fn find_source_dir() -> Option<PathBuf> {
    // 1. Cheapest check: env var
    if let Ok(env_dir) = std::env::var("AGT_DIR").or_else(|_| std::env::var("AGENT_SKILLS_DIR")) {
        let p = PathBuf::from(env_dir);
        if p.is_dir() {
            return Some(p);
        }
    }

    // 2. Walk up from executable following symlinks
    if let Ok(exe) = std::env::current_exe() {
        let resolved = fs::canonicalize(&exe).unwrap_or(exe);
        let mut dir = resolved.parent();
        for _ in 0..5 {
            match dir {
                Some(d) => {
                    if has_skill_groups(d) {
                        return Some(d.to_path_buf());
                    }
                    dir = d.parent();
                }
                None => break,
            }
        }
    }

    // 3. Fallback: check common install locations
    if let Some(home) = dirs::home_dir() {
        for candidate in &[".agent-skills", ".agt", "agt"] {
            let p = home.join(candidate);
            if has_skill_groups(&p) {
                return Some(p);
            }
        }
    }

    None
}

/// Check if CWD (or its git root) is itself a skills source directory.
/// This detects when the user is inside a skills repo like `agent-skills`.
pub fn find_cwd_source_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    if has_skill_groups(&cwd) {
        return Some(cwd);
    }
    // Walk up to git root and check there too
    if let Some(root) = git_root() {
        if root != cwd && has_skill_groups(&root) {
            return Some(root);
        }
    }
    None
}

/// Hint message when no source directory is found
pub fn source_dir_hint() -> String {
    let home = dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_else(|| "~".to_string());
    format!(
        "No skills found. Install skills with:\n  \
         git clone https://github.com/jiunbae/agent-skills {home}/.agent-skills\n  \
         or set AGT_DIR to your skills directory"
    )
}

pub fn has_skill_groups(dir: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if !is_excluded(&name_str) && !name_str.starts_with('.') {
                    // Check if any subdirectory contains SKILL.md
                    if let Ok(sub_entries) = fs::read_dir(&path) {
                        for sub in sub_entries.flatten() {
                            if sub.path().is_dir() && sub.path().join("SKILL.md").exists() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Check if a directory name is in the exclude list
pub fn is_excluded(name: &str) -> bool {
    EXCLUDE_DIRS.contains(&name)
}

/// Get all skill group names from the source directory
pub fn skill_groups(source_dir: &Path) -> Vec<String> {
    let mut groups = Vec::new();
    if let Ok(entries) = fs::read_dir(source_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if is_excluded(&name_str) || name_str.starts_with('.') {
                continue;
            }
            // Verify at least one skill exists
            if !skills_in_group(source_dir, &name_str).is_empty() {
                groups.push(name_str);
            }
        }
    }
    groups.sort();
    groups
}

/// Get all skill names in a group
pub fn skills_in_group(source_dir: &Path, group: &str) -> Vec<String> {
    let group_dir = source_dir.join(group);
    let mut skills = Vec::new();
    if let Ok(entries) = fs::read_dir(&group_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("SKILL.md").exists() {
                if let Some(name) = entry.file_name().to_str() {
                    skills.push(name.to_string());
                }
            }
        }
    }
    skills.sort();
    skills
}

/// Find the git repository root by walking up from cwd
pub fn git_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Skill target directories
pub fn local_skill_target() -> PathBuf {
    git_root()
        .map(|r| r.join(".claude/skills"))
        .unwrap_or_else(|| PathBuf::from(".claude/skills"))
}

pub fn global_skill_target() -> PathBuf {
    claude_config_dir().join("skills")
}

pub fn global_team_target() -> PathBuf {
    claude_config_dir().join("teams")
}

pub fn local_codex_skill_target() -> PathBuf {
    git_root()
        .map(|r| r.join(".agents/skills"))
        .unwrap_or_else(|| PathBuf::from(".agents/skills"))
}

pub fn global_codex_skill_target() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".agents/skills")
}

pub fn skill_target(global: bool, agent: SkillAgent) -> PathBuf {
    match (global, agent) {
        (false, SkillAgent::Claude) => local_skill_target(),
        (true, SkillAgent::Claude) => global_skill_target(),
        (false, SkillAgent::Codex) => local_codex_skill_target(),
        (true, SkillAgent::Codex) => global_codex_skill_target(),
    }
}

/// Claude Code and Codex both discover only direct children of their skills
/// directory (`<dir>/<skill>/SKILL.md`), so every destination is flat.
pub fn skill_destination(target_dir: &Path, skill_name: &str) -> PathBuf {
    target_dir.join(skill_name)
}

/// Claude Code skill directory names: lowercase letters, digits and hyphens.
pub fn is_skill_dir_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Directory name a skill installs under: its frontmatter `name` when that is a
/// valid skill name, else its source directory name. Repositories group skills
/// by service (`billing/notion`, `crm/notion`), so directory names repeat
/// across groups while the declared names stay unique.
pub fn install_name(skill_path: &Path, dir_name: &str) -> String {
    fs::read_to_string(skill_path.join("SKILL.md"))
        .ok()
        .and_then(|content| crate::frontmatter::get_field(&content, "name"))
        .map(|name| name.trim().trim_matches(['"', '\'']).to_string())
        .filter(|name| is_skill_dir_name(name))
        .unwrap_or_else(|| dir_name.to_string())
}

/// Where agt before 2026.10 put Claude skills: `<target>/<group>/<skill>`.
/// Claude Code never loads skills from there; kept only for migration.
pub fn legacy_grouped_destination(target_dir: &Path, group: &str, skill_name: &str) -> PathBuf {
    target_dir.join(group).join(skill_name)
}

/// Persona paths
pub fn persona_library(source_dir: &Path) -> PathBuf {
    source_dir.join("personas")
}

pub fn local_persona_target() -> PathBuf {
    git_root()
        .map(|r| r.join(".agents/personas"))
        .unwrap_or_else(|| PathBuf::from(".agents/personas"))
}

pub fn global_persona_target() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".agents/personas")
}

/// Hook paths
pub fn global_hook_target() -> PathBuf {
    claude_config_dir().join("hooks")
}

pub fn claude_settings_path() -> PathBuf {
    claude_config_dir().join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_is_flat() {
        assert_eq!(
            skill_destination(Path::new("/tmp/skills"), "git-commit-pr"),
            PathBuf::from("/tmp/skills/git-commit-pr")
        );
    }

    #[test]
    fn legacy_destination_keeps_group() {
        assert_eq!(
            legacy_grouped_destination(Path::new("/tmp/skills"), "development", "git-commit-pr"),
            PathBuf::from("/tmp/skills/development/git-commit-pr")
        );
    }

    #[test]
    fn install_name_prefers_frontmatter_name() {
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join("billing/notion");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "---\nname: billing-notion\n---\n").unwrap();
        assert_eq!(install_name(&skill, "notion"), "billing-notion");

        fs::write(skill.join("SKILL.md"), "---\nname: Not Valid\n---\n").unwrap();
        assert_eq!(install_name(&skill, "notion"), "notion");
    }

    #[test]
    fn claude_dir_flag_wins_over_env() {
        assert_eq!(
            resolve_claude_config_dir(
                Some(Path::new("/flag")),
                Some(OsString::from("/env")),
                Some(PathBuf::from("/home/u")),
            ),
            PathBuf::from("/flag")
        );
    }

    #[test]
    fn claude_dir_env_wins_over_home() {
        assert_eq!(
            resolve_claude_config_dir(
                None,
                Some(OsString::from("/env")),
                Some(PathBuf::from("/home/u"))
            ),
            PathBuf::from("/env")
        );
    }

    #[test]
    fn claude_dir_empty_env_falls_back_to_home() {
        assert_eq!(
            resolve_claude_config_dir(None, Some(OsString::new()), Some(PathBuf::from("/home/u"))),
            PathBuf::from("/home/u/.claude")
        );
    }
}

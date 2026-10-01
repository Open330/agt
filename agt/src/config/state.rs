use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const STATE_VERSION: u32 = 1;

/// What agt installed into one skills directory. Lets agt tell its own
/// skills apart from ones a user placed by hand, and say which layer each
/// came from. Lives beside the skills directory, e.g. `~/.claude/agt-state.json`.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct InstallState {
    pub version: u32,
    /// Stack last applied by `agt apply`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillRecord {
    /// Why it is here: a profile name, `<source>:<profile>` for a stack
    /// layer, or `manual` for a single-skill install.
    pub layer: String,
    /// Local source directory or `owner/repo` the skill came from.
    pub source: String,
    /// `group/skill` inside the source.
    pub origin: String,
    pub mode: InstallMode,
    /// Installed by `agt apply`, which may also remove it again. Other
    /// installs are never pruned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub applied: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallMode {
    Symlink,
    Copy,
}

/// Beside the skills directory, after resolving symlinks, so every config dir
/// that shares one skills directory also shares one state file.
pub fn state_path(skills_dir: &Path) -> PathBuf {
    let resolved = fs::canonicalize(skills_dir).unwrap_or_else(|_| skills_dir.to_path_buf());
    match resolved.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join("agt-state.json"),
        _ => PathBuf::from("agt-state.json"),
    }
}

impl InstallState {
    pub fn load(skills_dir: &Path) -> Result<Self> {
        let path = state_path(skills_dir);
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content)
                .with_context(|| format!("Invalid agt state file {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                version: STATE_VERSION,
                ..Self::default()
            }),
            Err(e) => Err(e).with_context(|| format!("Failed to read {}", path.display())),
        }
    }

    pub fn save(&mut self, skills_dir: &Path) -> Result<()> {
        let path = state_path(skills_dir);
        self.version = STATE_VERSION;
        if self.skills.is_empty() && self.stack.is_none() {
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn record(&mut self, name: &str, record: SkillRecord) {
        self.skills.insert(name.to_string(), record);
    }

    pub fn forget(&mut self, name: &str) -> bool {
        self.skills.remove(name).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(layer: &str) -> SkillRecord {
        SkillRecord {
            layer: layer.to_string(),
            source: "/src".to_string(),
            origin: "dev/a".to_string(),
            mode: InstallMode::Symlink,
            applied: false,
        }
    }

    #[test]
    fn state_sits_beside_skills_dir() {
        assert_eq!(
            state_path(Path::new("/home/u/.claude/skills")),
            PathBuf::from("/home/u/.claude/agt-state.json")
        );
    }

    #[test]
    fn missing_file_loads_empty_state() {
        let tmp = tempfile::tempdir().unwrap();
        let state = InstallState::load(&tmp.path().join("skills")).unwrap();
        assert!(state.skills.is_empty());
    }

    #[test]
    fn round_trips_and_removes_file_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");

        let mut state = InstallState::load(&skills).unwrap();
        state.record("a", record("core"));
        state.save(&skills).unwrap();
        assert_eq!(
            InstallState::load(&skills).unwrap().skills["a"],
            record("core")
        );

        let mut state = InstallState::load(&skills).unwrap();
        assert!(state.forget("a"));
        state.save(&skills).unwrap();
        assert!(!state_path(&skills).exists());
    }
}

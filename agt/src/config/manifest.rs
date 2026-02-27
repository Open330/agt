use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_FILE: &str = "agt.toml";

#[derive(Debug, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub setup: Setup,
}

#[derive(Debug, Deserialize, Default)]
pub struct Setup {
    #[serde(default)]
    pub copy: Vec<SetupCopy>,
}

#[derive(Debug, Deserialize)]
pub struct SetupCopy {
    pub from: String,
    pub to: String,
    #[serde(default = "default_strategy")]
    pub strategy: String,
}

fn default_strategy() -> String {
    "merge".to_string()
}

/// Parse agt.toml from a directory. Returns None if the file doesn't exist.
pub fn parse_manifest(dir: &Path) -> Result<Option<Manifest>> {
    let manifest_path = dir.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&manifest_path)
        .context(format!("Failed to read {}", manifest_path.display()))?;

    let manifest: Manifest =
        toml::from_str(&content).context(format!("Invalid {}", manifest_path.display()))?;

    // Validate all copy rules
    for rule in &manifest.setup.copy {
        validate_copy_rule(rule)?;
    }

    Ok(Some(manifest))
}

/// Validate a copy rule for safety.
fn validate_copy_rule(rule: &SetupCopy) -> Result<()> {
    // Source must not escape the repo
    if rule.from.contains("..") || rule.from.starts_with('/') {
        bail!(
            "Invalid setup.copy.from '{}': must be a relative path within the repo",
            rule.from
        );
    }

    // Target must start with ~ (home-relative)
    if !rule.to.starts_with("~/") && !rule.to.starts_with("~\\") && rule.to != "~" {
        bail!(
            "Invalid setup.copy.to '{}': must be a home-relative path (~/...)",
            rule.to
        );
    }

    // No path traversal in target
    if rule.to.contains("..") {
        bail!(
            "Invalid setup.copy.to '{}': path traversal not allowed",
            rule.to
        );
    }

    // Validate strategy
    match rule.strategy.as_str() {
        "merge" | "replace" => {}
        other => bail!("Invalid strategy '{}': must be 'merge' or 'replace'", other),
    }

    Ok(())
}

/// Resolve ~ in a path to the actual home directory.
pub fn resolve_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest)
    } else if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"))
    } else {
        PathBuf::from(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(
            tmp.path().join("agt.toml"),
            r#"
[[setup.copy]]
from = "static"
to = "~/.agents"
strategy = "merge"

[[setup.copy]]
from = "personas"
to = "~/.agents/personas"
"#,
        )
        .unwrap();

        let manifest = parse_manifest(tmp.path()).unwrap().unwrap();
        assert_eq!(manifest.setup.copy.len(), 2);
        assert_eq!(manifest.setup.copy[0].from, "static");
        assert_eq!(manifest.setup.copy[0].to, "~/.agents");
        assert_eq!(manifest.setup.copy[0].strategy, "merge");
        assert_eq!(manifest.setup.copy[1].strategy, "merge"); // default
    }

    #[test]
    fn test_parse_manifest_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(parse_manifest(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn test_validate_path_traversal_source() {
        let rule = SetupCopy {
            from: "../etc".to_string(),
            to: "~/.agents".to_string(),
            strategy: "merge".to_string(),
        };
        assert!(validate_copy_rule(&rule).is_err());
    }

    #[test]
    fn test_validate_absolute_source() {
        let rule = SetupCopy {
            from: "/etc/passwd".to_string(),
            to: "~/.agents".to_string(),
            strategy: "merge".to_string(),
        };
        assert!(validate_copy_rule(&rule).is_err());
    }

    #[test]
    fn test_validate_non_home_target() {
        let rule = SetupCopy {
            from: "static".to_string(),
            to: "/tmp/evil".to_string(),
            strategy: "merge".to_string(),
        };
        assert!(validate_copy_rule(&rule).is_err());
    }

    #[test]
    fn test_validate_target_traversal() {
        let rule = SetupCopy {
            from: "static".to_string(),
            to: "~/../etc".to_string(),
            strategy: "merge".to_string(),
        };
        assert!(validate_copy_rule(&rule).is_err());
    }

    #[test]
    fn test_validate_invalid_strategy() {
        let rule = SetupCopy {
            from: "static".to_string(),
            to: "~/.agents".to_string(),
            strategy: "yolo".to_string(),
        };
        assert!(validate_copy_rule(&rule).is_err());
    }

    #[test]
    fn test_resolve_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolve_home("~/.agents"), home.join(".agents"));
        assert_eq!(resolve_home("~/foo/bar"), home.join("foo/bar"));
        assert_eq!(resolve_home("~"), home);
    }
}

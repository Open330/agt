//! Consumer manifest: the `[env]`, `[sources]`, and `[skills]` tables of
//! `agt.toml`. The `[setup]` table of the same file belongs to skill source
//! repositories (`config::manifest`) and is never executed by `agt sync`.

use crate::config::SkillAgent;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "agt.toml";
pub const LOCK_FILE: &str = "agt.lock";

/// Where a manifest lives and which skill directories it owns.
#[derive(Debug, Clone)]
pub struct Scope {
    pub global: bool,
    pub dir: PathBuf,
}

impl Scope {
    pub fn project() -> Result<Self> {
        let dir = match crate::config::git_root() {
            Some(root) => root,
            None => std::env::current_dir().context("Failed to read current directory")?,
        };
        Ok(Self { global: false, dir })
    }

    pub fn user() -> Result<Self> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .context("Cannot determine home directory")?;
        Ok(Self {
            global: true,
            dir: base.join("agt"),
        })
    }

    pub fn resolve(global: bool) -> Result<Self> {
        if global {
            Self::user()
        } else {
            Self::project()
        }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.dir.join(MANIFEST_FILE)
    }

    pub fn lock_path(&self) -> PathBuf {
        self.dir.join(LOCK_FILE)
    }

    /// Ownership id recorded in `.agt-managed`. Project skill directories live
    /// inside the repository, so they need no path (which would break when the
    /// checkout moves); user skills are owned by the user manifest.
    pub fn owner(&self) -> String {
        if self.global {
            self.manifest_path().display().to_string()
        } else {
            "project".to_string()
        }
    }

    pub fn flag(&self) -> &'static str {
        if self.global {
            " -g"
        } else {
            ""
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvManifest {
    #[serde(default)]
    pub env: EnvSettings,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceDef>,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillDep>,
    /// Owned by skill source repositories; parsed only so it is not rejected.
    #[serde(default)]
    #[allow(dead_code)]
    setup: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvSettings {
    #[serde(default = "default_agents")]
    pub agents: Vec<SkillAgent>,
}

impl Default for EnvSettings {
    fn default() -> Self {
        Self {
            agents: default_agents(),
        }
    }
}

fn default_agents() -> Vec<SkillAgent> {
    vec![SkillAgent::Claude]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDef {
    pub github: String,
    pub rev: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDep {
    pub source: Option<String>,
    pub github: Option<String>,
    pub rev: Option<String>,
    pub path: Option<String>,
    pub agents: Option<Vec<SkillAgent>>,
}

/// A skill dependency with its source alias resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDep {
    pub name: String,
    pub repo: String,
    pub rev: Option<String>,
    pub path: Option<String>,
    pub agents: Vec<SkillAgent>,
}

impl EnvManifest {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
        };
        let manifest: Self =
            toml::from_str(&content).with_context(|| format!("Invalid {}", path.display()))?;
        Ok(Some(manifest))
    }

    pub fn deps(&self) -> Result<Vec<ResolvedDep>> {
        self.skills
            .iter()
            .map(|(name, dep)| self.resolve_dep(name, dep))
            .collect()
    }

    fn resolve_dep(&self, name: &str, dep: &SkillDep) -> Result<ResolvedDep> {
        crate::util::validate_name(name)?;
        let (repo, source_rev) = match (&dep.source, &dep.github) {
            (Some(_), Some(_)) => bail!("skills.{name}: set either `source` or `github`, not both"),
            (Some(alias), None) => {
                let source = self
                    .sources
                    .get(alias)
                    .with_context(|| format!("skills.{name}: unknown source '{alias}'"))?;
                (source.github.clone(), source.rev.clone())
            }
            (None, Some(github)) => (github.clone(), None),
            (None, None) => bail!(
                "skills.{name}: missing `source` or `github` (local paths are not supported yet)"
            ),
        };
        validate_repo(&repo).with_context(|| format!("skills.{name}"))?;
        let path = dep.path.as_deref().map(|p| p.trim_matches('/').to_string());
        if let Some(path) = &path {
            crate::remote::validate_source_path(path)
                .with_context(|| format!("skills.{name}.path"))?;
        }
        let agents = dep
            .agents
            .clone()
            .unwrap_or_else(|| self.env.agents.clone());
        if agents.is_empty() {
            bail!("skills.{name}: `agents` cannot be empty");
        }
        Ok(ResolvedDep {
            name: name.to_string(),
            repo,
            rev: dep.rev.clone().or(source_rev),
            path,
            agents,
        })
    }
}

pub fn validate_repo(repo: &str) -> Result<()> {
    let parts: Vec<&str> = repo.split('/').collect();
    let valid_part = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if parts.len() != 2 || !parts.iter().all(|p| valid_part(p)) {
        bail!("Invalid GitHub repository '{repo}': expected owner/repo");
    }
    Ok(())
}

/// Format-preserving edits for `agt add` / `agt remove`.
pub mod edit {
    use super::*;
    use toml_edit::{value, Array, DocumentMut, InlineTable, Item, Table};

    pub fn load(path: &Path) -> Result<DocumentMut> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
        };
        content
            .parse::<DocumentMut>()
            .with_context(|| format!("Invalid {}", path.display()))
    }

    pub fn save(path: &Path, doc: &DocumentMut) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, doc.to_string())
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn starter(agents: &[SkillAgent]) -> DocumentMut {
        let mut doc = DocumentMut::new();
        let mut env = Table::new();
        let mut list = Array::new();
        for agent in agents {
            list.push(agent.to_string());
        }
        env["agents"] = value(list);
        doc["env"] = Item::Table(env);
        doc["skills"] = Item::Table(Table::new());
        doc
    }

    pub fn add_skill(
        doc: &mut DocumentMut,
        name: &str,
        repo: &str,
        rev: Option<&str>,
        path: Option<&str>,
    ) -> Result<()> {
        let skills = doc
            .entry("skills")
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_mut()
            .context("`skills` in agt.toml must be a table")?;
        if skills.contains_key(name) {
            bail!("Skill '{name}' is already declared in agt.toml");
        }
        let mut dep = InlineTable::new();
        dep.insert("github", repo.into());
        if let Some(rev) = rev {
            dep.insert("rev", rev.into());
        }
        if let Some(path) = path {
            dep.insert("path", path.into());
        }
        skills.insert(name, value(dep));
        Ok(())
    }

    pub fn remove_skill(doc: &mut DocumentMut, name: &str) -> bool {
        doc.get_mut("skills")
            .and_then(Item::as_table_like_mut)
            .and_then(|skills| skills.remove(name))
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> EnvManifest {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn resolves_source_alias_and_defaults() {
        let m = parse(
            r#"
[env]
agents = ["claude", "codex"]

[sources]
open330 = { github = "jiunbae/agent-skills", rev = "v1" }

[skills]
git-commit-pr = { source = "open330", path = "development/git-commit-pr/" }
pdf = { github = "anthropics/skills", rev = "main", agents = ["claude"] }
"#,
        );
        let deps = m.deps().unwrap();
        assert_eq!(
            deps[0],
            ResolvedDep {
                name: "git-commit-pr".into(),
                repo: "jiunbae/agent-skills".into(),
                rev: Some("v1".into()),
                path: Some("development/git-commit-pr".into()),
                agents: vec![SkillAgent::Claude, SkillAgent::Codex],
            }
        );
        assert_eq!(deps[1].agents, vec![SkillAgent::Claude]);
    }

    #[test]
    fn setup_table_is_tolerated() {
        let m = parse("[[setup.copy]]\nfrom = \"static\"\nto = \"~/.agents\"\n");
        assert!(m.skills.is_empty());
    }

    #[test]
    fn rejects_unknown_source_and_bad_repo() {
        let m = parse("[skills]\nx = { source = \"nope\" }\n");
        assert!(m.deps().is_err());
        let m = parse("[skills]\nx = { github = \"../evil\" }\n");
        assert!(m.deps().is_err());
        let m = parse("[skills]\nx = { github = \"a/b\", path = \"../up\" }\n");
        assert!(m.deps().is_err());
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(
            toml::from_str::<EnvManifest>("[skills]\nx = { github = \"a/b\", pin = \"1\" }\n")
                .is_err()
        );
    }

    #[test]
    fn edit_round_trip_preserves_comments() {
        let mut doc: toml_edit::DocumentMut = "# team env\n[skills]\n".parse().unwrap();
        edit::add_skill(&mut doc, "pdf", "anthropics/skills", Some("main"), None).unwrap();
        assert!(edit::add_skill(&mut doc, "pdf", "anthropics/skills", None, None).is_err());
        let text = doc.to_string();
        assert!(text.starts_with("# team env"));
        assert!(text.contains(r#"pdf = { github = "anthropics/skills", rev = "main" }"#));
        assert!(edit::remove_skill(&mut doc, "pdf"));
        assert!(!edit::remove_skill(&mut doc, "pdf"));
    }
}

//! Discover installed skills across agt-known locations and gh's host list.

use crate::config::SkillAgent;
use crate::environment::integrity::split_frontmatter;
use crate::environment::Marker;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    Project,
    User,
}

/// Where an installed skill came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Origin {
    /// Installed by `agt sync`; `owner` is the `.agt-managed` owner id.
    Agt {
        owner: String,
        integrity: String,
    },
    /// Installed by `gh skill install` (frontmatter `metadata.github-*`).
    Gh {
        repo: String,
        path: String,
        rev: Option<String>,
    },
    /// Installed by legacy `agt skill install --from` (`.remote-source`).
    RemoteSource {
        repo: String,
        path: String,
        rev: Option<String>,
    },
    /// A symlink into a local checkout (agt's local install, or hand-made).
    Link {
        target: PathBuf,
    },
    Unknown,
}

impl Origin {
    /// `(owner/repo, path, rev)` when the skill can be traced back to GitHub.
    pub fn github(&self) -> Option<(&str, &str, Option<&str>)> {
        match self {
            Origin::Gh { repo, path, rev } | Origin::RemoteSource { repo, path, rev } => {
                Some((repo, path, rev.as_deref()))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledSkill {
    /// Agent host: claude, codex, or a gh host id for other agents.
    pub agent: String,
    pub scope: ScopeKind,
    /// Directory name, which agents use as the skill id.
    pub name: String,
    pub path: PathBuf,
    #[serde(skip)]
    pub real: Option<PathBuf>,
    pub origin: Origin,
    /// `<group>/<skill>` under a Claude skills dir, which Claude Code never loads.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub grouped: bool,
    #[serde(skip)]
    pub skill_md: Option<String>,
}

impl InstalledSkill {
    pub fn is_broken_link(&self) -> bool {
        self.real.is_none()
    }

    /// Whether the agent actually loads this copy.
    pub fn is_loaded(&self) -> bool {
        !self.grouped && !self.is_broken_link()
    }
}

pub struct ScanRoot {
    pub agent: SkillAgent,
    pub scope: ScopeKind,
    pub dir: PathBuf,
}

pub fn default_roots() -> Vec<ScanRoot> {
    let mut roots = Vec::new();
    if crate::config::git_root().is_some() {
        for agent in [SkillAgent::Claude, SkillAgent::Codex] {
            roots.push(ScanRoot {
                agent,
                scope: ScopeKind::Project,
                dir: crate::config::skill_target(false, agent),
            });
        }
    }
    for agent in [SkillAgent::Claude, SkillAgent::Codex] {
        roots.push(ScanRoot {
            agent,
            scope: ScopeKind::User,
            dir: crate::config::skill_target(true, agent),
        });
    }
    roots
}

pub fn scan(roots: &[ScanRoot]) -> Vec<InstalledSkill> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(&root.dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if path.is_symlink() && !path.exists() {
                out.push(skill(root, name, path, false));
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            if path.join("SKILL.md").is_file() {
                out.push(skill(root, name, path, false));
            } else if root.agent == SkillAgent::Claude {
                // Legacy agt grouped layout: <group>/<skill>/SKILL.md. Listed so
                // doctor can point at `agt skill migrate`; Claude Code ignores it.
                let Ok(children) = fs::read_dir(&path) else {
                    continue;
                };
                let mut children: Vec<_> = children.flatten().map(|e| e.path()).collect();
                children.sort();
                for child in children {
                    if child.join("SKILL.md").is_file() {
                        let name = child.file_name().unwrap().to_string_lossy().into_owned();
                        out.push(skill(root, name, child, true));
                    }
                }
            }
        }
    }
    out
}

fn skill(root: &ScanRoot, name: String, path: PathBuf, grouped: bool) -> InstalledSkill {
    let real = fs::canonicalize(&path).ok();
    let skill_md = fs::read_to_string(path.join("SKILL.md")).ok();
    let origin = origin(&path, skill_md.as_deref());
    InstalledSkill {
        agent: root.agent.to_string(),
        scope: root.scope,
        name,
        path,
        real,
        origin,
        grouped,
        skill_md,
    }
}

pub fn origin(dir: &Path, skill_md: Option<&str>) -> Origin {
    if let Some(marker) = Marker::read(dir) {
        return Origin::Agt {
            owner: marker.owner,
            integrity: marker.integrity,
        };
    }
    if let Some(origin) = skill_md.and_then(gh_origin) {
        return origin;
    }
    if let Ok(spec) = crate::remote::parse_metadata(dir) {
        return Origin::RemoteSource {
            repo: format!("{}/{}", spec.owner, spec.repo),
            path: spec.path,
            rev: Some(spec.git_ref),
        };
    }
    if dir.is_symlink() {
        if let Ok(target) = fs::read_link(dir) {
            return Origin::Link { target };
        }
    }
    Origin::Unknown
}

fn gh_origin(skill_md: &str) -> Option<Origin> {
    let (yaml, _) = split_frontmatter(skill_md)?;
    let fm: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    let meta = fm.get("metadata")?;
    let field = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let url = field("github-repo")?;
    let repo = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .split_once("github.com/")?
        .1
        .to_string();
    let rev = field("github-pinned").or_else(|| {
        field("github-ref").map(|r| {
            r.strip_prefix("refs/heads/")
                .or_else(|| r.strip_prefix("refs/tags/"))
                .unwrap_or(&r)
                .to_string()
        })
    });
    Some(Origin::Gh {
        repo,
        path: field("github-path")?,
        rev,
    })
}

/// Skills gh knows about in other agent hosts' directories. Best effort: an
/// absent or failing `gh` simply contributes nothing.
pub fn gh_hosts(seen: &[InstalledSkill]) -> Vec<InstalledSkill> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GhListed {
        path: PathBuf,
        skill_name: String,
        scope: String,
        agent_hosts: Vec<String>,
    }
    let Ok(output) = Command::new("gh")
        .args(["skill", "list", "--json", "path,skillName,scope,agentHosts"])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(listed) = serde_json::from_slice::<Vec<GhListed>>(&output.stdout) else {
        return Vec::new();
    };
    let known: BTreeSet<PathBuf> = seen
        .iter()
        .flat_map(|s| [Some(s.path.clone()), s.real.clone()])
        .flatten()
        .collect();
    listed
        .into_iter()
        .filter(|l| {
            !known.contains(&l.path)
                && !fs::canonicalize(&l.path).is_ok_and(|real| known.contains(&real))
        })
        .map(|l| {
            let scope = if l.scope == "project" {
                ScopeKind::Project
            } else {
                ScopeKind::User
            };
            let skill_md = fs::read_to_string(l.path.join("SKILL.md")).ok();
            InstalledSkill {
                agent: l.agent_hosts.first().cloned().unwrap_or_default(),
                scope,
                name: l.skill_name,
                real: fs::canonicalize(&l.path).ok(),
                origin: origin(&l.path, skill_md.as_deref()),
                grouped: false,
                skill_md,
                path: l.path,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gh_metadata() {
        let md = "---\nname: pdf\nmetadata:\n    github-path: skills/pdf\n    github-ref: refs/tags/v1.2\n    github-repo: https://github.com/anthropics/skills\n---\nbody";
        assert_eq!(
            gh_origin(md),
            Some(Origin::Gh {
                repo: "anthropics/skills".into(),
                path: "skills/pdf".into(),
                rev: Some("v1.2".into()),
            })
        );
        let pinned = md.replace("    github-ref", "    github-pinned: abc\n    github-ref");
        assert!(matches!(gh_origin(&pinned), Some(Origin::Gh { rev: Some(r), .. }) if r == "abc"));
        assert_eq!(gh_origin("---\nname: x\n---\n"), None);
    }

    #[test]
    fn scans_flat_grouped_and_broken_links() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("skills");
        for dir in ["flat", "group/nested"] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("SKILL.md"), "---\nname: x\n---\n").unwrap();
        }
        std::os::unix::fs::symlink(tmp.path().join("missing"), root.join("dangling")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();

        let found = scan(&[ScanRoot {
            agent: SkillAgent::Claude,
            scope: ScopeKind::User,
            dir: root.clone(),
        }]);
        let names: Vec<_> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["dangling", "flat", "nested"]);
        assert!(found[0].is_broken_link());
        assert!(!found[1].grouped && found[2].grouped);
        assert!(!found[2].is_loaded());

        // Codex discovers only direct children.
        let codex = scan(&[ScanRoot {
            agent: SkillAgent::Codex,
            scope: ScopeKind::User,
            dir: root,
        }]);
        assert_eq!(codex.len(), 2);
    }
}

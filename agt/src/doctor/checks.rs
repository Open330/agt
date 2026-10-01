use super::scan::{InstalledSkill, Origin, ScopeKind};
use crate::environment::integrity;
use crate::environment::lock::Lockfile;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: &'static str,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Finding {
    fn new(id: &'static str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            id,
            severity,
            skill: None,
            path: None,
            message: message.into(),
            hint: None,
        }
    }

    fn at(mut self, skill: &InstalledSkill) -> Self {
        self.skill = Some(skill.name.clone());
        self.path = Some(skill.path.clone());
        self
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// A manifest scope agt manages (project or user).
pub struct ManagedScope {
    pub kind: ScopeKind,
    pub owner: String,
    pub flag: &'static str,
    pub has_manifest: bool,
    pub lock: Option<Lockfile>,
}

pub struct Options {
    /// Token budget for all skill descriptions one agent loads per session.
    pub budget: usize,
}

pub fn run(skills: &[InstalledSkill], scopes: &[ManagedScope], opts: &Options) -> Vec<Finding> {
    let mut findings = Vec::new();
    for skill in skills {
        if skill.is_broken_link() {
            findings.push(
                Finding::new("D7", Severity::Error, "symlink target does not exist")
                    .at(skill)
                    .hint("remove the link or restore its target"),
            );
            continue;
        }
        lock_drift(skill, scopes, &mut findings);
        frontmatter(skill, &mut findings);
        scripts(skill, &mut findings);
    }
    duplicates(skills, &mut findings);
    unmanaged(skills, scopes, &mut findings);
    budget(skills, opts.budget, &mut findings);
    findings.sort_by(|a, b| {
        (a.severity, a.id, &a.skill, &a.path).cmp(&(b.severity, b.id, &b.skill, &b.path))
    });
    findings
}

/// D1: agt-managed skills must match agt.lock.
fn lock_drift(skill: &InstalledSkill, scopes: &[ManagedScope], out: &mut Vec<Finding>) {
    let Origin::Agt { owner, .. } = &skill.origin else {
        return;
    };
    let Some(scope) = scopes.iter().find(|s| &s.owner == owner) else {
        out.push(
            Finding::new(
                "D1",
                Severity::Warn,
                format!("managed by a manifest outside this check ({owner})"),
            )
            .at(skill),
        );
        return;
    };
    let locked = scope.lock.as_ref().and_then(|lock| lock.skill(&skill.name));
    let Some(locked) = locked else {
        out.push(
            Finding::new(
                "D1",
                Severity::Warn,
                "installed by agt but missing from agt.lock",
            )
            .at(skill)
            .hint(format!("agt sync{} removes it", scope.flag)),
        );
        return;
    };
    match integrity::hash_dir(&skill.path) {
        Ok(hash) if hash == locked.integrity => {}
        Ok(_) => out.push(
            Finding::new(
                "D1",
                Severity::Error,
                "modified since install (does not match agt.lock)",
            )
            .at(skill)
            .hint(format!(
                "agt sync{} restores the locked content",
                scope.flag
            )),
        ),
        Err(e) => {
            out.push(Finding::new("D1", Severity::Error, format!("cannot hash: {e:#}")).at(skill))
        }
    }
}

/// D6: frontmatter per the Agent Skills spec.
fn frontmatter(skill: &InstalledSkill, out: &mut Vec<Finding>) {
    let Some(md) = &skill.skill_md else {
        out.push(Finding::new("D6", Severity::Error, "SKILL.md is unreadable").at(skill));
        return;
    };
    let Some((yaml, _)) = integrity::split_frontmatter(md) else {
        out.push(Finding::new("D6", Severity::Error, "SKILL.md has no frontmatter").at(skill));
        return;
    };
    let fm: serde_yaml::Value = match serde_yaml::from_str(yaml) {
        Ok(fm) => fm,
        Err(e) => {
            out.push(
                Finding::new("D6", Severity::Error, format!("invalid frontmatter: {e}")).at(skill),
            );
            return;
        }
    };
    let field = |k: &str| fm.get(k).and_then(|v| v.as_str()).map(str::trim);

    match field("name") {
        None | Some("") => {
            out.push(Finding::new("D6", Severity::Error, "missing `name`").at(skill))
        }
        Some(name) => {
            let valid = name.len() <= 64
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !name.starts_with('-')
                && !name.ends_with('-');
            if !valid {
                out.push(
                    Finding::new(
                        "D6",
                        Severity::Warn,
                        format!("`name: {name}` is not lowercase-hyphenated (max 64)"),
                    )
                    .at(skill),
                );
            } else if name != skill.name {
                out.push(
                    Finding::new(
                        "D6",
                        Severity::Warn,
                        format!("`name: {name}` does not match its directory"),
                    )
                    .at(skill)
                    .hint("agents may list it under either id; keep them equal"),
                );
            }
        }
    }

    match field("description") {
        None | Some("") => out.push(
            Finding::new(
                "D6",
                Severity::Error,
                "missing `description` (the skill cannot trigger)",
            )
            .at(skill),
        ),
        Some(desc) if desc.chars().count() > 1024 => out.push(
            Finding::new(
                "D6",
                Severity::Warn,
                format!(
                    "description is {} chars (spec max 1024)",
                    desc.chars().count()
                ),
            )
            .at(skill),
        ),
        Some(desc) if desc.chars().count() < 20 => out.push(
            Finding::new(
                "D6",
                Severity::Warn,
                "description is too short to trigger reliably",
            )
            .at(skill),
        ),
        _ => {}
    }
}

/// D7: scripts with a shebang that cannot be executed.
fn scripts(skill: &InstalledSkill, out: &mut Vec<Finding>) {
    let mut missing = Vec::new();
    walk_files(&skill.path, &skill.path, 0, &mut |rel, path| {
        let Ok(meta) = fs::metadata(path) else { return };
        if meta.permissions().mode() & 0o111 != 0 {
            return;
        }
        let mut head = [0u8; 2];
        if let Ok(mut f) = fs::File::open(path) {
            use std::io::Read;
            if f.read_exact(&mut head).is_ok() && &head == b"#!" {
                missing.push(rel.to_string());
            }
        }
    });
    if !missing.is_empty() {
        missing.sort();
        let shown = missing
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = missing.len().saturating_sub(3);
        let suffix = if more > 0 {
            format!(" (+{more})")
        } else {
            String::new()
        };
        out.push(
            Finding::new(
                "D7",
                Severity::Warn,
                format!("scripts are not executable: {shown}{suffix}"),
            )
            .at(skill)
            .hint(match &skill.origin {
                Origin::Gh { .. } => format!(
                    "`gh skill install` drops exec bits; `agt adopt {}` reinstalls with them",
                    skill.name
                ),
                Origin::Link { target } => format!("chmod +x them in {}", target.display()),
                _ => "chmod +x them if they are meant to be run directly".to_string(),
            }),
        );
    }
}

fn walk_files(root: &Path, dir: &Path, depth: usize, f: &mut dyn FnMut(&str, &Path)) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk_files(root, &path, depth + 1, f);
        } else if ft.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                f(&rel.to_string_lossy(), &path);
            }
        }
    }
}

/// D2: one agent sees the same skill id more than once.
fn duplicates(skills: &[InstalledSkill], out: &mut Vec<Finding>) {
    let mut groups: BTreeMap<(&str, &str), Vec<&InstalledSkill>> = BTreeMap::new();
    for skill in skills.iter().filter(|s| !s.is_broken_link()) {
        groups
            .entry((&skill.agent, &skill.name))
            .or_default()
            .push(skill);
    }
    for ((agent, name), group) in groups {
        let distinct: BTreeSet<_> = group.iter().map(|s| s.real.clone()).collect();
        if distinct.len() < 2 {
            continue;
        }
        let places = group
            .iter()
            .map(|s| s.path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        out.push(Finding {
            skill: Some(name.to_string()),
            ..Finding::new(
                "D2",
                Severity::Warn,
                format!("{agent} sees {} copies: {places}", group.len()),
            )
            .hint("keep one; which copy wins differs between agents")
        });
    }
}

/// D9: skills installed outside the manifest of a scope that has one.
fn unmanaged(skills: &[InstalledSkill], scopes: &[ManagedScope], out: &mut Vec<Finding>) {
    for scope in scopes.iter().filter(|s| s.has_manifest) {
        for skill in skills.iter().filter(|s| {
            s.scope == scope.kind
                && matches!(s.agent.as_str(), "claude" | "codex")
                && !matches!(s.origin, Origin::Agt { .. })
        }) {
            let hint = if skill.origin.github().is_some() {
                format!("agt adopt{} {}", scope.flag, skill.name)
            } else {
                "declare it in agt.toml, or remove it".to_string()
            };
            out.push(
                Finding::new("D9", Severity::Info, "not declared in agt.toml")
                    .at(skill)
                    .hint(hint),
            );
        }
    }
}

/// Rough token estimate: ~4 ASCII chars per token, ~1 token per other char.
pub fn estimate_tokens(text: &str) -> usize {
    let (ascii, other) = text.chars().fold((0usize, 0usize), |(a, o), c| {
        if c.is_ascii() {
            (a + 1, o)
        } else {
            (a, o + 1)
        }
    });
    ascii.div_ceil(4) + other
}

/// D4: description text every session preloads, per agent.
fn budget(skills: &[InstalledSkill], limit: usize, out: &mut Vec<Finding>) {
    let mut per_agent: BTreeMap<&str, BTreeMap<PathBuf, (&str, usize)>> = BTreeMap::new();
    for skill in skills.iter().filter(|s| !s.is_broken_link()) {
        let Some(md) = &skill.skill_md else { continue };
        let Some((yaml, _)) = integrity::split_frontmatter(md) else {
            continue;
        };
        let fm: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap_or_default();
        let text = ["name", "description"]
            .iter()
            .filter_map(|k| fm.get(*k).and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        let key = skill.real.clone().unwrap_or_else(|| skill.path.clone());
        per_agent
            .entry(&skill.agent)
            .or_default()
            .insert(key, (&skill.name, estimate_tokens(&text)));
    }
    for (agent, entries) in per_agent {
        let total: usize = entries.values().map(|(_, t)| t).sum();
        let mut top: Vec<_> = entries.values().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let top = top
            .iter()
            .take(3)
            .map(|(name, t)| format!("{name} {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let severity = if total > limit {
            Severity::Warn
        } else {
            Severity::Info
        };
        out.push(Finding::new(
            "D4",
            severity,
            format!(
                "{agent}: {} skills preload ≈{total} tokens of descriptions (budget {limit}); largest: {top}",
                entries.len()
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SkillAgent;
    use crate::doctor::scan::{scan, ScanRoot};
    use crate::environment::lock::LockedPackage;

    fn write(dir: &Path, md: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), md).unwrap();
    }

    fn ids(findings: &[Finding], skill: &str) -> Vec<&'static str> {
        findings
            .iter()
            .filter(|f| f.skill.as_deref() == Some(skill))
            .map(|f| f.id)
            .collect()
    }

    #[test]
    fn reports_each_problem_class() {
        let tmp = tempfile::TempDir::new().unwrap();
        let user = tmp.path().join("user");
        let project = tmp.path().join("project");
        let good =
            "---\nname: good\ndescription: Use when the user asks for something good.\n---\n";

        write(&user.join("good"), good);
        write(&user.join("dup"), &good.replace("good", "dup"));
        write(&project.join("dup"), &good.replace("good", "dup"));
        write(&user.join("noname"), "---\ndescription: short\n---\n");
        write(
            &user.join("mismatch"),
            &good.replace("name: good", "name: other"),
        );
        write(&user.join("script"), &good.replace("good", "script"));
        fs::write(user.join("script/run.sh"), "#!/bin/sh\necho\n").unwrap();
        write(&user.join("managed"), &good.replace("good", "managed"));
        fs::write(
            user.join("managed/.agt-managed"),
            "owner: /cfg/agt.toml\nname: managed\nintegrity: x\n",
        )
        .unwrap();
        write(
            &project.join("fromgh"),
            &good.replace(
                "---\nname",
                "---\nmetadata:\n  github-repo: https://github.com/a/b\n  github-path: s/fromgh\nname",
            ).replace("good", "fromgh"),
        );

        let skills = scan(&[
            ScanRoot {
                agent: SkillAgent::Claude,
                scope: ScopeKind::Project,
                dir: project,
            },
            ScanRoot {
                agent: SkillAgent::Claude,
                scope: ScopeKind::User,
                dir: user,
            },
        ]);
        let mut lock = Lockfile::new();
        lock.packages.push(LockedPackage {
            kind: "skill".into(),
            name: "managed".into(),
            source: "github:a/b".into(),
            rev: "main".into(),
            commit: String::new(),
            path: "managed".into(),
            tree: String::new(),
            integrity: "agt1-sha256-different".into(),
            executables: vec![],
        });
        let scopes = [
            ManagedScope {
                kind: ScopeKind::Project,
                owner: "project".into(),
                flag: "",
                has_manifest: true,
                lock: None,
            },
            ManagedScope {
                kind: ScopeKind::User,
                owner: "/cfg/agt.toml".into(),
                flag: " -g",
                has_manifest: true,
                lock: Some(lock),
            },
        ];
        let findings = run(&skills, &scopes, &Options { budget: 10 });

        assert_eq!(ids(&findings, "good"), ["D9"]);
        assert_eq!(ids(&findings, "noname"), ["D6", "D6", "D9"]);
        assert_eq!(ids(&findings, "mismatch"), ["D6", "D9"]);
        assert_eq!(ids(&findings, "script"), ["D7", "D9"]);
        assert_eq!(ids(&findings, "managed"), ["D1"]);
        assert!(ids(&findings, "dup").contains(&"D2"));
        let adopt = findings
            .iter()
            .find(|f| f.skill.as_deref() == Some("fromgh"))
            .unwrap();
        assert_eq!(adopt.hint.as_deref(), Some("agt adopt fromgh"));
        let d4 = findings.iter().find(|f| f.id == "D4").unwrap();
        assert_eq!(d4.severity, Severity::Warn);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn token_estimate_counts_cjk_heavier() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("한국어"), 3);
    }
}

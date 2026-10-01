//! Content hash for installed skills.
//!
//! `gh skill install` rewrites SKILL.md frontmatter (injects `metadata.github-*`
//! and reorders keys), so raw bytes cannot be compared with the source. The
//! hash therefore normalizes SKILL.md: drop gh's tracking keys, then serialize
//! the remaining frontmatter with sorted keys before the body. Every other file
//! contributes its relative path, executable bit, and content hash.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub const MANAGED_MARKER: &str = ".agt-managed";
const PREFIX: &str = "agt1-sha256-";

pub fn hash_dir(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for rel in &files {
        let path = dir.join(rel);
        let bytes =
            fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?;
        let content = if rel == "SKILL.md" {
            normalize_skill_md(&bytes)?
        } else {
            bytes
        };
        let exec = fs::metadata(&path)?.permissions().mode() & 0o111 != 0;
        hasher.update(rel.as_bytes());
        hasher.update([0, exec as u8, 0]);
        hasher.update(Sha256::digest(&content));
    }
    Ok(format!("{PREFIX}{}", hex(&hasher.finalize())))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect(root, &path, out)?;
        } else if entry.file_name() != MANAGED_MARKER {
            let rel = path.strip_prefix(root).expect("walk stays under root");
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

fn normalize_skill_md(bytes: &[u8]) -> Result<Vec<u8>> {
    let text = String::from_utf8_lossy(bytes);
    let Some((yaml, body)) = split_frontmatter(&text) else {
        return Ok(bytes.to_vec());
    };
    let mut fm: serde_yaml::Value =
        serde_yaml::from_str(yaml).context("Invalid SKILL.md frontmatter")?;
    if let Some(map) = fm.as_mapping_mut() {
        let drop_metadata = match map.get_mut("metadata").and_then(|m| m.as_mapping_mut()) {
            Some(metadata) => {
                metadata.retain(|k, _| !k.as_str().is_some_and(|k| k.starts_with("github-")));
                metadata.is_empty()
            }
            None => false,
        };
        if drop_metadata {
            map.remove("metadata");
        }
    }
    // serde_json maps are BTreeMaps here, so this yields sorted keys.
    let canonical = serde_json::to_string(&serde_json::to_value(&fm)?)?;
    Ok(format!("{canonical}\n{}", body.trim_start_matches('\n')).into_bytes())
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim_start().strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let body = &rest[end + 4..];
    Some((&rest[..end], body.split_once('\n').map_or("", |(_, b)| b)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(dir: &Path, skill_md: &str) {
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("SKILL.md"), skill_md).unwrap();
        fs::write(dir.join("scripts/run.sh"), "echo hi\n").unwrap();
    }

    #[test]
    fn ignores_gh_metadata_and_key_order() {
        let tmp = tempfile::TempDir::new().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        write_skill(
            &a,
            "---\nname: pdf\ndescription: Use for PDFs\n---\n# Body\n",
        );
        write_skill(
            &b,
            "---\ndescription: Use for PDFs\nmetadata:\n    github-repo: https://github.com/a/b\n    github-tree-sha: abc\nname: pdf\n---\n# Body\n",
        );
        fs::write(b.join(MANAGED_MARKER), "name: pdf\n").unwrap();
        assert_eq!(hash_dir(&a).unwrap(), hash_dir(&b).unwrap());
    }

    #[test]
    fn keeps_non_gh_metadata() {
        let tmp = tempfile::TempDir::new().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        write_skill(&a, "---\nname: x\nmetadata:\n  owner: team\n---\nbody\n");
        write_skill(&b, "---\nname: x\n---\nbody\n");
        assert_ne!(hash_dir(&a).unwrap(), hash_dir(&b).unwrap());
    }

    #[test]
    fn detects_content_and_mode_changes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let a = tmp.path().join("a");
        write_skill(&a, "---\nname: x\n---\nbody\n");
        let before = hash_dir(&a).unwrap();
        let script = a.join("scripts/run.sh");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let after_chmod = hash_dir(&a).unwrap();
        assert_ne!(before, after_chmod);
        fs::write(&script, "rm -rf /\n").unwrap();
        assert_ne!(after_chmod, hash_dir(&a).unwrap());
    }
}

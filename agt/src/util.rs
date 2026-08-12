use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

/// Validate a skill/persona name to prevent path traversal and catch argument mistakes
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        bail!("Name contains invalid path characters: {}", name);
    }
    if name == "." || name == ".." || name.starts_with("..") {
        bail!("Name cannot be a relative path component: {}", name);
    }
    // Catch likely argument order mistakes: names with spaces are probably descriptions
    if name.contains(' ') {
        bail!(
            "Name '{}' contains spaces. Did you mean to put the name before the flag?\n  \
             Example: agt persona create my-name --codex \"description here\"",
            name
        );
    }
    Ok(())
}

/// Check if target path exists and clear it if force is set
pub fn ensure_target_clear(path: &Path, force: bool, entity_name: &str) -> Result<()> {
    if path.exists() || path.is_symlink() {
        if force {
            if path.is_symlink() || path.is_file() {
                fs::remove_file(path)?;
            } else {
                fs::remove_dir_all(path)?;
            }
        } else {
            bail!(
                "'{}' already installed at {}. Use --force to overwrite.",
                entity_name,
                path.display()
            );
        }
    }
    Ok(())
}

/// Create a replacement symlink before touching the live entry, then activate it with rollback.
pub fn replace_symlink_transactionally(src: &Path, dst: &Path) -> Result<()> {
    replace_symlink_transactionally_with(
        src,
        dst,
        |target, candidate| symlink(target, candidate),
        |from, to| fs::rename(from, to),
    )
}

fn replace_symlink_transactionally_with<C, R>(
    src: &Path,
    dst: &Path,
    create_candidate: C,
    mut rename: R,
) -> Result<()>
where
    C: FnOnce(&Path, &Path) -> std::io::Result<()>,
    R: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let parent = dst
        .parent()
        .context("Symlink replacement destination has no parent directory")?;
    let reserved_candidate = tempfile::Builder::new()
        .prefix(".agt-link-candidate-")
        .tempfile_in(parent)
        .context("Failed to reserve a symlink replacement candidate")?;
    let candidate = reserved_candidate.path().to_path_buf();
    reserved_candidate
        .close()
        .context("Failed to prepare a symlink replacement candidate")?;

    let result = (|| {
        create_candidate(src, &candidate).with_context(|| {
            format!(
                "Failed to create symlink replacement candidate {} -> {}",
                candidate.display(),
                src.display()
            )
        })?;

        let metadata = fs::symlink_metadata(&candidate).with_context(|| {
            format!(
                "Failed to validate symlink replacement candidate {}",
                candidate.display()
            )
        })?;
        if !metadata.file_type().is_symlink() || fs::read_link(&candidate)? != src {
            bail!(
                "Symlink replacement candidate failed validation: {}",
                candidate.display()
            );
        }

        if !dst.exists() && !dst.is_symlink() {
            return rename(&candidate, dst).with_context(|| {
                format!(
                    "Failed to activate replacement symlink at {}",
                    dst.display()
                )
            });
        }

        let recovery_root = tempfile::Builder::new()
            .prefix(".agt-link-recovery-")
            .tempdir_in(parent)
            .context("Failed to create symlink replacement recovery directory")?;
        let recovery_path = recovery_root.path().join("previous");

        rename(dst, &recovery_path).with_context(|| {
            format!(
                "Failed to preserve existing entry before replacing {}",
                dst.display()
            )
        })?;

        if let Err(activation_error) = rename(&candidate, dst) {
            return match rename(&recovery_path, dst) {
                Ok(()) => Err(anyhow::anyhow!(activation_error)).with_context(|| {
                    format!(
                        "Failed to activate replacement symlink at {}; previous entry was restored",
                        dst.display()
                    )
                }),
                Err(rollback_error) => {
                    let retained_root = recovery_root.keep();
                    let retained_path = retained_root.join("previous");
                    bail!(
                        "Failed to activate replacement symlink at {}: {}; rollback failed: {}; previous entry retained at {}",
                        dst.display(),
                        activation_error,
                        rollback_error,
                        retained_path.display()
                    )
                }
            };
        }

        drop(recovery_root);
        Ok(())
    })();

    if candidate.is_symlink() || candidate.is_file() {
        let _ = fs::remove_file(&candidate);
    } else if candidate.is_dir() {
        let _ = fs::remove_dir_all(&candidate);
    }
    result
}

/// Recursively copy a directory, skipping symlinks for safety
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        // Skip symlinks to prevent exfiltration of files outside the source tree
        if file_type.is_symlink() {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Stage a directory replacement next to its destination, then activate it with rollback.
///
/// `prepare` can add metadata or perform final validation. It completes before the live
/// destination is touched. Symlinks inside `src` retain `copy_dir_recursive`'s safety policy.
pub fn replace_dir_transactionally<F>(src: &Path, dst: &Path, prepare: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    let parent = dst
        .parent()
        .context("Replacement destination has no parent directory")?;
    let staging_root = tempfile::Builder::new()
        .prefix(".agt-skill-stage-")
        .tempdir_in(parent)
        .context("Failed to create skill staging directory")?;
    let staged = staging_root.path().join("candidate");

    copy_dir_recursive(src, &staged).context("Failed to stage skill replacement")?;
    prepare(&staged).context("Failed to prepare staged skill replacement")?;

    activate_staged_dir(&staged, dst)
}

fn activate_staged_dir(staged: &Path, dst: &Path) -> Result<()> {
    activate_staged_dir_with(staged, dst, |from, to| fs::rename(from, to))
}

fn activate_staged_dir_with<R>(staged: &Path, dst: &Path, mut rename: R) -> Result<()>
where
    R: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    if !staged.is_dir() {
        bail!(
            "Staged skill replacement is not a directory: {}",
            staged.display()
        );
    }

    if !dst.exists() && !dst.is_symlink() {
        return rename(staged, dst).with_context(|| {
            format!(
                "Failed to activate staged skill replacement at {}",
                dst.display()
            )
        });
    }

    let parent = dst
        .parent()
        .context("Replacement destination has no parent directory")?;
    let recovery_root = tempfile::Builder::new()
        .prefix(".agt-skill-recovery-")
        .tempdir_in(parent)
        .context("Failed to create skill recovery directory")?;
    let recovery_path = recovery_root.path().join("previous");

    rename(dst, &recovery_path).with_context(|| {
        format!(
            "Failed to preserve existing skill before replacing {}",
            dst.display()
        )
    })?;

    if let Err(activation_error) = rename(staged, dst) {
        return match rename(&recovery_path, dst) {
            Ok(()) => Err(anyhow::anyhow!(activation_error)).with_context(|| {
                format!(
                    "Failed to activate staged skill replacement at {}; previous installation was restored",
                    dst.display()
                )
            }),
            Err(rollback_error) => {
                let retained_root = recovery_root.keep();
                let retained_path = retained_root.join("previous");
                bail!(
                    "Failed to activate staged skill replacement at {}: {}; rollback failed: {}; previous installation retained at {}",
                    dst.display(),
                    activation_error,
                    rollback_error,
                    retained_path.display()
                )
            }
        };
    }

    // Keep the recoverable old tree until the new tree has been activated successfully.
    drop(recovery_root);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        activate_staged_dir_with, replace_dir_transactionally, replace_symlink_transactionally,
        replace_symlink_transactionally_with,
    };
    use anyhow::bail;
    use std::fs;

    #[test]
    fn forced_symlink_replacement_builds_candidate_before_touching_live_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let old_target = temp.path().join("old-target");
        let new_target = temp.path().join("new-target");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&old_target).unwrap();
        fs::create_dir_all(&new_target).unwrap();
        std::os::unix::fs::symlink(&old_target, &destination).unwrap();

        replace_symlink_transactionally_with(
            &new_target,
            &destination,
            |target, candidate| {
                assert_eq!(fs::read_link(&destination).unwrap(), old_target);
                std::os::unix::fs::symlink(target, candidate)
            },
            |from, to| fs::rename(from, to),
        )
        .unwrap();

        assert_eq!(fs::read_link(destination).unwrap(), new_target);
    }

    #[test]
    fn symlink_candidate_creation_failure_leaves_existing_entry_untouched() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::write(&destination, "old bytes").unwrap();

        let error = replace_symlink_transactionally_with(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("injected candidate failure")),
            |from, to| fs::rename(from, to),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected candidate failure"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old bytes");
    }

    #[test]
    fn invalid_symlink_candidate_leaves_existing_entry_untouched() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("sentinel"), "old bytes").unwrap();

        let error = replace_symlink_transactionally_with(
            &source,
            &destination,
            |_, candidate| fs::write(candidate, "not a link"),
            |from, to| fs::rename(from, to),
        )
        .unwrap_err();

        assert!(error.to_string().contains("failed validation"));
        assert_eq!(
            fs::read_to_string(destination.join("sentinel")).unwrap(),
            "old bytes"
        );
    }

    #[test]
    fn symlink_activation_failure_restores_previous_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::write(&destination, "old bytes").unwrap();
        let mut rename_count = 0;

        let error = replace_symlink_transactionally_with(
            &source,
            &destination,
            |target, candidate| std::os::unix::fs::symlink(target, candidate),
            |from, to| {
                rename_count += 1;
                if rename_count == 2 {
                    Err(std::io::Error::other("injected activation failure"))
                } else {
                    fs::rename(from, to)
                }
            },
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("previous entry was restored"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old bytes");
    }

    #[test]
    fn symlink_rollback_failure_reports_and_retains_exact_recovery_path() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::write(&destination, "old bytes").unwrap();
        let mut rename_count = 0;

        let error = replace_symlink_transactionally_with(
            &source,
            &destination,
            |target, candidate| std::os::unix::fs::symlink(target, candidate),
            |from, to| {
                rename_count += 1;
                if rename_count >= 2 {
                    Err(std::io::Error::other("injected rename failure"))
                } else {
                    fs::rename(from, to)
                }
            },
        )
        .unwrap_err();
        let message = format!("{error:#}");
        let retained = message
            .split("previous entry retained at ")
            .nth(1)
            .map(std::path::PathBuf::from)
            .unwrap();

        assert_eq!(fs::read_to_string(&retained).unwrap(), "old bytes");
        assert!(retained.starts_with(temp.path()));
        assert!(!destination.exists());
    }

    #[test]
    fn forced_symlink_replacement_activates_when_destination_is_absent() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();

        replace_symlink_transactionally(&source, &destination).unwrap();

        assert_eq!(fs::read_link(destination).unwrap(), source);
    }

    #[test]
    fn copy_staging_failure_leaves_existing_directory_untouched() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing_source = temp.path().join("missing-source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), "old bytes").unwrap();
        fs::write(destination.join(".remote-source"), "old metadata").unwrap();

        let error =
            replace_dir_transactionally(&missing_source, &destination, |_| Ok(())).unwrap_err();

        assert!(format!("{error:#}").contains("Failed to stage skill replacement"));
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "old bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "old metadata"
        );
    }

    #[test]
    fn metadata_staging_failure_leaves_existing_directory_untouched() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("SKILL.md"), "new bytes").unwrap();
        fs::write(destination.join("SKILL.md"), "old bytes").unwrap();
        fs::write(destination.join("keep.txt"), "keep me").unwrap();
        fs::write(destination.join(".remote-source"), "old metadata").unwrap();

        let error = replace_dir_transactionally(&source, &destination, |_| {
            bail!("injected metadata failure")
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected metadata failure"));
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "old bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join("keep.txt")).unwrap(),
            "keep me"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "old metadata"
        );
    }

    #[test]
    fn successful_replacement_activates_staged_bytes_and_metadata() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("SKILL.md"), "new bytes").unwrap();
        fs::write(source.join("new.txt"), "new file").unwrap();
        fs::write(destination.join("SKILL.md"), "old bytes").unwrap();
        fs::write(destination.join("old.txt"), "old file").unwrap();

        replace_dir_transactionally(&source, &destination, |staged| {
            fs::write(staged.join(".remote-source"), "source: test/repo/skill\n")?;
            Ok(())
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "new bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join("new.txt")).unwrap(),
            "new file"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "source: test/repo/skill\n"
        );
        assert!(!destination.join("old.txt").exists());
    }

    #[test]
    fn activation_failure_restores_previous_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let staged = temp.path().join("staged");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(staged.join("SKILL.md"), "new bytes").unwrap();
        fs::write(destination.join("SKILL.md"), "old bytes").unwrap();
        let mut rename_count = 0;

        let error = activate_staged_dir_with(&staged, &destination, |from, to| {
            rename_count += 1;
            if rename_count == 2 {
                Err(std::io::Error::other("injected activation failure"))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains("previous installation was restored"));
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "old bytes"
        );
        assert_eq!(
            fs::read_to_string(staged.join("SKILL.md")).unwrap(),
            "new bytes"
        );
    }

    #[test]
    fn rollback_failure_reports_and_retains_recovery_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let staged = temp.path().join("staged");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(staged.join("SKILL.md"), "new bytes").unwrap();
        fs::write(destination.join("SKILL.md"), "old bytes").unwrap();
        let mut rename_count = 0;

        let error = activate_staged_dir_with(&staged, &destination, |from, to| {
            rename_count += 1;
            if rename_count >= 2 {
                Err(std::io::Error::other("injected rename failure"))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("previous installation retained at"));
        let recovery = fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".agt-skill-recovery-"))
            })
            .unwrap()
            .join("previous");
        assert_eq!(
            fs::read_to_string(recovery.join("SKILL.md")).unwrap(),
            "old bytes"
        );
    }
}

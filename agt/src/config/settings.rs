use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_json_atomically<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    // Serialization must succeed before creating directories or temporary files.
    let content = serde_json::to_vec_pretty(value).context("Cannot serialize JSON settings")?;
    write_serialized_atomically_with(path, &content, &mut FileSystemOperations)
}

fn write_serialized_atomically_with(
    path: &Path,
    content: &[u8],
    operations: &mut impl AtomicWriteOperations,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Cannot create settings directory {}", parent.display()))?;

    let permissions = existing_permissions(path)?;
    let (mut temporary, temporary_path) = create_temporary_file(path, parent)?;
    let mut cleanup = TemporaryFileCleanup::new(temporary_path.clone());

    operations
        .write_content(&mut temporary, content)
        .with_context(|| {
            format!(
                "Cannot write temporary settings file {}",
                temporary_path.display()
            )
        })?;
    operations.flush(&mut temporary).with_context(|| {
        format!(
            "Cannot flush temporary settings file {}",
            temporary_path.display()
        )
    })?;
    operations.sync_file(&temporary).with_context(|| {
        format!(
            "Cannot sync temporary settings file {}",
            temporary_path.display()
        )
    })?;

    if let Some(permissions) = permissions {
        operations
            .set_permissions(&temporary, permissions)
            .with_context(|| {
                format!(
                    "Cannot preserve permissions on temporary settings file {}",
                    temporary_path.display()
                )
            })?;
        operations.sync_file(&temporary).with_context(|| {
            format!(
                "Cannot sync permissions on temporary settings file {}",
                temporary_path.display()
            )
        })?;
    }
    drop(temporary);

    operations
        .promote(&temporary_path, path)
        .with_context(|| format!("Cannot atomically replace settings file {}", path.display()))?;
    cleanup.disarm();

    // Promotion is the commit point. A directory sync can improve crash
    // durability, but once rename succeeds there is no safe way to restore the
    // original bytes. Do not report a false write failure after the commit.
    let _ = operations.sync_parent(parent);
    Ok(())
}

trait AtomicWriteOperations {
    fn write_content(&mut self, file: &mut File, content: &[u8]) -> io::Result<()>;
    fn flush(&mut self, file: &mut File) -> io::Result<()>;
    fn sync_file(&mut self, file: &File) -> io::Result<()>;
    fn set_permissions(&mut self, file: &File, permissions: Permissions) -> io::Result<()>;
    fn promote(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    fn sync_parent(&mut self, parent: &Path) -> io::Result<()>;
}

struct FileSystemOperations;

impl AtomicWriteOperations for FileSystemOperations {
    fn write_content(&mut self, file: &mut File, content: &[u8]) -> io::Result<()> {
        file.write_all(content)
    }

    fn flush(&mut self, file: &mut File) -> io::Result<()> {
        file.flush()
    }

    fn sync_file(&mut self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn set_permissions(&mut self, file: &File, permissions: Permissions) -> io::Result<()> {
        file.set_permissions(permissions)
    }

    fn promote(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn sync_parent(&mut self, parent: &Path) -> io::Result<()> {
        sync_parent_directory(parent)
    }
}

fn existing_permissions(path: &Path) -> Result<Option<Permissions>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("Cannot inspect settings file {}", path.display()))
        }
    }
}

fn create_temporary_file(path: &Path, parent: &Path) -> Result<(File, PathBuf)> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.json");

    for _ in 0..128 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{file_name}.agt-{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        match options.open(&temporary_path) {
            Ok(file) => return Ok((file, temporary_path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Cannot create temporary settings file in {}",
                        parent.display()
                    )
                })
            }
        }
    }

    anyhow::bail!(
        "Cannot allocate a unique temporary settings file in {}",
        parent.display()
    )
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    File::open(parent).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

struct TemporaryFileCleanup {
    path: Option<PathBuf>,
}

impl TemporaryFileCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TemporaryFileCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::{Error as _, Serializer};

    struct SerializationFailure;

    impl Serialize for SerializationFailure {
        fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("injected serialization failure"))
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FailureStage {
        Write,
        Flush,
        FileSync,
        Permission,
        Promote,
        ParentSync,
    }

    struct TestOperations {
        failure: FailureStage,
        parent_sync_attempted: bool,
    }

    impl TestOperations {
        fn failing(failure: FailureStage) -> Self {
            Self {
                failure,
                parent_sync_attempted: false,
            }
        }

        fn injected_failure(stage: &str) -> io::Error {
            io::Error::other(format!("injected {stage} failure"))
        }
    }

    impl AtomicWriteOperations for TestOperations {
        fn write_content(&mut self, file: &mut File, content: &[u8]) -> io::Result<()> {
            if self.failure == FailureStage::Write {
                file.write_all(&content[..content.len().min(4)])?;
                return Err(Self::injected_failure("write"));
            }
            file.write_all(content)
        }

        fn flush(&mut self, file: &mut File) -> io::Result<()> {
            if self.failure == FailureStage::Flush {
                return Err(Self::injected_failure("flush"));
            }
            file.flush()
        }

        fn sync_file(&mut self, file: &File) -> io::Result<()> {
            if self.failure == FailureStage::FileSync {
                return Err(Self::injected_failure("file sync"));
            }
            file.sync_all()
        }

        fn set_permissions(&mut self, file: &File, permissions: Permissions) -> io::Result<()> {
            if self.failure == FailureStage::Permission {
                return Err(Self::injected_failure("permission"));
            }
            file.set_permissions(permissions)
        }

        fn promote(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            if self.failure == FailureStage::Promote {
                return Err(Self::injected_failure("promotion"));
            }
            fs::rename(from, to)
        }

        fn sync_parent(&mut self, parent: &Path) -> io::Result<()> {
            self.parent_sync_attempted = true;
            if self.failure == FailureStage::ParentSync {
                return Err(Self::injected_failure("parent sync"));
            }
            sync_parent_directory(parent)
        }
    }

    fn assert_no_temporary_files(directory: &Path) {
        let entries = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["settings.json"]);
    }

    #[test]
    fn serialization_failure_does_not_mutate_the_filesystem() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing_parent = temp.path().join("missing");
        let settings_path = missing_parent.join("settings.json");

        let error = write_json_atomically(&settings_path, &SerializationFailure).unwrap_err();

        assert!(format!("{error:#}").contains("injected serialization failure"));
        assert!(!missing_parent.exists());
    }

    #[test]
    fn temporary_write_failure_preserves_original_and_cleans_up() {
        let temp = tempfile::TempDir::new().unwrap();
        let settings_path = temp.path().join("settings.json");
        let original = br#"{"valid":true}"#;
        fs::write(&settings_path, original).unwrap();

        let error = write_serialized_atomically_with(
            &settings_path,
            br#"{"replacement":true}"#,
            &mut TestOperations::failing(FailureStage::Write),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected write failure"));
        assert_eq!(fs::read(&settings_path).unwrap(), original);
        assert_no_temporary_files(temp.path());
    }

    #[test]
    fn flush_sync_permission_and_promotion_failures_preserve_original_and_clean_up() {
        for (stage, expected) in [
            (FailureStage::Flush, "injected flush failure"),
            (FailureStage::FileSync, "injected file sync failure"),
            (FailureStage::Permission, "injected permission failure"),
            (FailureStage::Promote, "injected promotion failure"),
        ] {
            let temp = tempfile::TempDir::new().unwrap();
            let settings_path = temp.path().join("settings.json");
            let original = br#"{"valid":true}"#;
            fs::write(&settings_path, original).unwrap();

            let error = write_serialized_atomically_with(
                &settings_path,
                br#"{"replacement":true}"#,
                &mut TestOperations::failing(stage),
            )
            .unwrap_err();

            assert!(format!("{error:#}").contains(expected));
            assert_eq!(fs::read(&settings_path).unwrap(), original);
            assert_no_temporary_files(temp.path());
        }
    }

    #[test]
    fn post_promotion_parent_sync_failure_is_not_reported_as_a_write_failure() {
        let temp = tempfile::TempDir::new().unwrap();
        let settings_path = temp.path().join("settings.json");
        fs::write(&settings_path, br#"{"valid":true}"#).unwrap();
        let replacement = br#"{"replacement":true}"#;
        let mut operations = TestOperations::failing(FailureStage::ParentSync);

        write_serialized_atomically_with(&settings_path, replacement, &mut operations).unwrap();

        assert!(operations.parent_sync_attempted);
        assert_eq!(fs::read(&settings_path).unwrap(), replacement);
        assert_no_temporary_files(temp.path());
    }

    #[test]
    fn successful_write_replaces_json_atomically() {
        let temp = tempfile::TempDir::new().unwrap();
        let settings_path = temp.path().join("settings.json");
        fs::write(&settings_path, br#"{"valid":true}"#).unwrap();

        write_json_atomically(&settings_path, &serde_json::json!({ "new": true })).unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({ "new": true }));
        assert_no_temporary_files(temp.path());
    }

    #[cfg(unix)]
    #[test]
    fn existing_permissions_are_preserved_and_new_files_are_restrictive() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let existing = temp.path().join("settings.json");
        fs::write(&existing, "{}").unwrap();
        fs::set_permissions(&existing, Permissions::from_mode(0o640)).unwrap();

        write_json_atomically(&existing, &serde_json::json!({ "new": true })).unwrap();
        assert_eq!(
            fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o640
        );

        let new_path = temp.path().join("new-settings.json");
        write_json_atomically(&new_path, &serde_json::json!({ "new": true })).unwrap();
        assert_eq!(
            fs::metadata(new_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

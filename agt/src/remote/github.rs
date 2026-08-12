use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use tar::Archive;
use tempfile::TempDir;

/// Resolve a GitHub token from environment or `gh auth token`.
fn get_github_token() -> Option<String> {
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Ok(t) = std::env::var("GH_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    // Fall back to gh CLI
    Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Build a ureq request with optional auth header.
fn authed_get(url: &str) -> ureq::Request {
    let req = ureq::get(url);
    if let Some(token) = get_github_token() {
        req.set("Authorization", &format!("Bearer {}", token))
    } else {
        req
    }
}

/// Parsed remote specification
#[derive(Debug, Clone)]
pub struct RemoteSpec {
    pub owner: String,
    pub repo: String,
    pub path: String,
    pub git_ref: String,
}

impl std::fmt::Display for RemoteSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}/{}@{}",
            self.owner, self.repo, self.path, self.git_ref
        )
    }
}

/// Parse "owner/repo/path[@ref]" into a RemoteSpec.
/// Also accepts URL-style input: github.com/owner/repo/path, https://github.com/owner/repo/path
pub fn parse_spec(spec: &str) -> Result<RemoteSpec> {
    let spec = spec.trim();
    // Strip common URL prefixes
    let spec = spec
        .strip_prefix("https://")
        .or_else(|| spec.strip_prefix("http://"))
        .unwrap_or(spec);
    let spec = spec.strip_prefix("github.com/").unwrap_or(spec);
    let spec = spec.trim_end_matches('/');

    // Extract @ref suffix
    let (path_part, git_ref) = if let Some(at_pos) = spec.rfind('@') {
        (&spec[..at_pos], spec[at_pos + 1..].to_string())
    } else {
        (spec, "main".to_string())
    };

    let parts: Vec<&str> = path_part.split('/').collect();
    if parts.len() < 2 {
        bail!(
            "Invalid format: {}\nExpected: owner/repo[/path/to/skill][@ref]\n\
             Examples:\n  jiunbae/agent-skills/agents/background-reviewer\n  \
             jiunbae/agent-skills  (browse all skills in repo)",
            spec
        );
    }

    Ok(RemoteSpec {
        owner: parts[0].to_string(),
        repo: parts[1].to_string(),
        path: if parts.len() > 2 {
            parts[2..].join("/")
        } else {
            String::new()
        },
        git_ref,
    })
}

/// Reject remote paths that could address content outside the selected repository tree.
/// An empty path is valid for repository-level operations.
pub fn validate_source_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    if path.contains('\\') || path.contains('\0') || Path::new(path).is_absolute() {
        bail!("Remote source path must be relative: {}", path);
    }

    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            bail!("Remote source path contains invalid component: {}", path);
        }
    }
    Ok(())
}

fn select_extracted_path(root: &Path, path: &str) -> Result<PathBuf> {
    validate_source_path(path)?;
    if root.is_symlink() {
        bail!("Remote archive root is a symlink: {}", root.display());
    }

    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("Failed to resolve remote archive root: {}", root.display()))?;
    if !canonical_root.is_dir() {
        bail!("Remote archive root is not a directory: {}", root.display());
    }

    let target = if path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(path)
    };
    let relative = target.strip_prefix(root).with_context(|| {
        format!(
            "Remote source path escapes archive root: {}",
            target.display()
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!("Remote source path is malformed: {}", path);
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("Remote source path not found: {}", current.display()))?;
        if metadata.file_type().is_symlink() {
            bail!(
                "Remote source path traverses a symlink: {}",
                current.display()
            );
        }
    }

    let canonical_target = fs::canonicalize(&target)
        .with_context(|| format!("Failed to resolve remote source path: {}", target.display()))?;
    if !canonical_target.starts_with(&canonical_root) {
        bail!(
            "Remote source path escapes archive root: {}",
            target.display()
        );
    }
    let metadata = fs::metadata(&canonical_target)?;
    if !metadata.is_dir() && !metadata.is_file() {
        bail!(
            "Remote source path is not a regular file or directory: {}",
            target.display()
        );
    }

    Ok(target)
}

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_TARBALL_SIZE: u64 = 50 * 1024 * 1024; // 50 MB compressed
const MAX_ARCHIVE_ENTRY_SIZE: u64 = 10 * 1024 * 1024; // 10 MB extracted
const MAX_EXTRACTED_SIZE: u64 = 100 * 1024 * 1024; // 100 MB extracted in total
const MAX_ARCHIVE_ENTRIES: u64 = 10_000;
const TAR_BLOCK_SIZE: u64 = 512;
const MAX_ARCHIVE_METADATA_SIZE: u64 = 16 * 1024 * 1024;
// Payload + one header and at most one padding block per entry + end markers + metadata.
const MAX_DECOMPRESSED_TARBALL_SIZE: u64 = MAX_EXTRACTED_SIZE
    + MAX_ARCHIVE_ENTRIES * (2 * TAR_BLOCK_SIZE)
    + 2 * TAR_BLOCK_SIZE
    + MAX_ARCHIVE_METADATA_SIZE;

#[derive(Clone, Copy)]
struct ArchiveLimits {
    max_archive_size: u64,
    max_entry_size: u64,
    max_extracted_size: u64,
    max_entries: u64,
}

const ARCHIVE_LIMITS: ArchiveLimits = ArchiveLimits {
    max_archive_size: MAX_DECOMPRESSED_TARBALL_SIZE,
    max_entry_size: MAX_ARCHIVE_ENTRY_SIZE,
    max_extracted_size: MAX_EXTRACTED_SIZE,
    max_entries: MAX_ARCHIVE_ENTRIES,
};

struct BoundedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> BoundedReader<R> {
    fn new(inner: R, max_size: u64) -> Self {
        Self {
            inner,
            remaining: max_size,
        }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe) {
                Ok(0) => Ok(0),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "decompressed tarball exceeds its byte limit",
                )),
                Err(error) => Err(error),
            };
        }

        let allowed = buffer
            .len()
            .min(usize::try_from(self.remaining).ok().unwrap_or(usize::MAX));
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn read_bounded(mut reader: impl Read, max_size: u64, resource: &str) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    reader
        .by_ref()
        .take(max_size + 1)
        .read_to_end(&mut body)
        .with_context(|| format!("Failed to read {resource}"))?;
    if body.len() as u64 > max_size {
        bail!("{resource} exceeds the {} byte limit", max_size);
    }
    Ok(body)
}

fn extract_archive(reader: impl Read, destination: &Path, limits: ArchiveLimits) -> Result<()> {
    let mut archive = Archive::new(BoundedReader::new(reader, limits.max_archive_size));
    let mut entry_count = 0u64;
    let mut extracted_size = 0u64;

    for entry in archive
        .entries()
        .context("Failed to read archive entries")?
    {
        let mut entry = entry.context("Failed to read archive entry")?;
        entry_count = entry_count
            .checked_add(1)
            .context("Archive entry count overflow")?;
        if entry_count > limits.max_entries {
            bail!("Archive exceeds the {} entry limit", limits.max_entries);
        }

        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() && !entry_type.is_dir() {
            bail!("Archive contains unsupported entry type");
        }

        let entry_size = entry
            .header()
            .size()
            .context("Invalid archive entry size")?;
        if entry_size > limits.max_entry_size {
            bail!(
                "Archive entry exceeds the {} byte limit",
                limits.max_entry_size
            );
        }
        extracted_size = extracted_size
            .checked_add(entry_size)
            .context("Archive extracted size overflow")?;
        if extracted_size > limits.max_extracted_size {
            bail!(
                "Archive exceeds the {} byte extracted-size limit",
                limits.max_extracted_size
            );
        }

        if !entry
            .unpack_in(destination)
            .context("Failed to extract archive entry")?
        {
            bail!("Archive entry path escapes the extraction directory");
        }
    }

    Ok(())
}

/// Download a single file from raw.githubusercontent.com
pub fn fetch_file(spec: &RemoteSpec) -> Result<Vec<u8>> {
    validate_source_path(&spec.path)?;
    if spec.path.is_empty() {
        bail!("Remote file path cannot be empty");
    }

    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_message(format!("Fetching {}...", spec.path));
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        spec.owner, spec.repo, spec.git_ref, spec.path
    );

    let response = authed_get(&url)
        .call()
        .context(format!("Failed to download: {}", url))?;

    let body = read_bounded(response.into_reader(), MAX_FILE_SIZE, "response")?;

    spinner.finish_and_clear();
    Ok(body)
}

/// Download a directory from GitHub tarball, extract to a temp directory.
/// Returns (TempDir, path_to_extracted_content).
/// The TempDir must be kept alive by the caller — dropping it cleans up.
pub fn fetch_dir(spec: &RemoteSpec) -> Result<(TempDir, PathBuf)> {
    validate_source_path(&spec.path)?;

    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_message(format!(
        "Downloading {}/{}@{}...",
        spec.owner, spec.repo, spec.git_ref
    ));
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let tmp_dir = TempDir::new().context("Failed to create temp directory")?;

    // Try API tarball (works with auth for private repos), then archive URLs
    let urls = [
        format!(
            "https://api.github.com/repos/{}/{}/tarball/{}",
            spec.owner, spec.repo, spec.git_ref
        ),
        format!(
            "https://github.com/{}/{}/archive/refs/tags/{}.tar.gz",
            spec.owner, spec.repo, spec.git_ref
        ),
        format!(
            "https://github.com/{}/{}/archive/refs/heads/{}.tar.gz",
            spec.owner, spec.repo, spec.git_ref
        ),
    ];

    let mut extracted_root: Option<PathBuf> = None;

    for url in &urls {
        // Clean previous attempt
        if let Ok(entries) = fs::read_dir(tmp_dir.path()) {
            for entry in entries.flatten() {
                let _ = fs::remove_dir_all(entry.path());
            }
        }

        let response = match authed_get(url)
            .set("Accept", "application/vnd.github+json")
            .set("User-Agent", "agt-cli")
            .call()
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let compressed = match read_bounded(
            response.into_reader(),
            MAX_TARBALL_SIZE,
            "compressed tarball",
        ) {
            Ok(compressed) => compressed,
            Err(_) => continue,
        };
        let decoder = GzDecoder::new(Cursor::new(compressed));

        if extract_archive(decoder, tmp_dir.path(), ARCHIVE_LIMITS).is_err() {
            continue;
        }

        // Find extracted root directory
        if let Ok(entries) = fs::read_dir(tmp_dir.path()) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    extracted_root = Some(entry.path());
                    break;
                }
            }
        }

        if extracted_root.is_some() {
            break;
        }
    }

    let root = extracted_root.context(format!(
        "Download failed: {}/{}@{}\n\
         If this is a private repo, ensure authentication is available:\n  \
         gh auth login          (gh CLI)\n  \
         GITHUB_TOKEN=<token>   (environment variable)",
        spec.owner, spec.repo, spec.git_ref
    ))?;

    let target_path = select_extracted_path(&root, &spec.path).with_context(|| {
        format!(
            "Invalid path '{}' in {}/{}@{}",
            spec.path, spec.owner, spec.repo, spec.git_ref
        )
    })?;

    spinner.finish_and_clear();
    Ok((tmp_dir, target_path))
}

/// Write .remote-source metadata file
pub fn write_metadata(target: &Path, spec: &RemoteSpec) -> Result<()> {
    let metadata_path = if target.is_dir() {
        target.join(".remote-source")
    } else {
        let stem = target
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        target.with_file_name(format!("{}.remote-source", stem))
    };

    let content = format!(
        "source: {}/{}/{}\nref: {}\ninstalled: {}\n",
        spec.owner,
        spec.repo,
        spec.path,
        spec.git_ref,
        chrono_like_now()
    );

    fs::write(&metadata_path, content).context("Failed to write remote metadata")?;
    Ok(())
}

/// Parse .remote-source metadata file back into a RemoteSpec.
pub fn parse_metadata(skill_dir: &Path) -> Result<RemoteSpec> {
    let metadata_path = skill_dir.join(".remote-source");
    let content = fs::read_to_string(&metadata_path)
        .context(format!("Failed to read {}", metadata_path.display()))?;

    let mut source = String::new();
    let mut git_ref = "main".to_string();

    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("source:") {
            source = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("ref:") {
            git_ref = val.trim().to_string();
        }
    }

    if source.is_empty() {
        bail!(
            "Invalid .remote-source: missing 'source' field in {}",
            metadata_path.display()
        );
    }

    // source is "owner/repo/path" — split into parts
    let parts: Vec<&str> = source.splitn(3, '/').collect();
    if parts.len() < 2 {
        bail!("Invalid source format in .remote-source: {}", source);
    }

    Ok(RemoteSpec {
        owner: parts[0].to_string(),
        repo: parts[1].to_string(),
        path: if parts.len() > 2 {
            parts[2].to_string()
        } else {
            String::new()
        },
        git_ref,
    })
}

fn chrono_like_now() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Simple UTC timestamp without chrono dependency
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let mins = (time_secs % 3600) / 60;
    let s = time_secs % 60;

    // Approximate date calculation (good enough for metadata)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let is_leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md as i64 {
            m = i + 1;
            break;
        }
        remaining_days -= md as i64;
    }
    if m == 0 {
        m = 12;
        remaining_days = 0;
    }
    let d = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, mins, s
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            for (path, contents) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(contents.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, path, *contents).unwrap();
            }
            builder.finish().unwrap();
        }
        bytes
    }

    #[test]
    fn test_parse_spec_basic() {
        let spec = parse_spec("jiunbae/agent-skills/agents/background-reviewer").unwrap();
        assert_eq!(spec.owner, "jiunbae");
        assert_eq!(spec.repo, "agent-skills");
        assert_eq!(spec.path, "agents/background-reviewer");
        assert_eq!(spec.git_ref, "main");
    }

    #[test]
    fn test_parse_spec_with_ref() {
        let spec =
            parse_spec("jiunbae/agent-skills/agents/background-reviewer@v2026.02.19.1").unwrap();
        assert_eq!(spec.path, "agents/background-reviewer");
        assert_eq!(spec.git_ref, "v2026.02.19.1");
    }

    #[test]
    fn test_parse_spec_persona() {
        let spec = parse_spec("jiunbae/agent-skills/personas/security-reviewer").unwrap();
        assert_eq!(spec.path, "personas/security-reviewer");
    }

    #[test]
    fn test_parse_spec_url_prefix() {
        let spec = parse_spec("https://github.com/jiunbae/agent-skills/agents/background-reviewer")
            .unwrap();
        assert_eq!(spec.owner, "jiunbae");
        assert_eq!(spec.repo, "agent-skills");
        assert_eq!(spec.path, "agents/background-reviewer");

        let spec = parse_spec("github.com/jiunbae/agent-skills/context/context-manager").unwrap();
        assert_eq!(spec.owner, "jiunbae");
        assert_eq!(spec.path, "context/context-manager");
    }

    #[test]
    fn test_parse_spec_repo_only() {
        let spec = parse_spec("jiunbae/agent-skills").unwrap();
        assert_eq!(spec.owner, "jiunbae");
        assert_eq!(spec.repo, "agent-skills");
        assert_eq!(spec.path, "");
        assert_eq!(spec.git_ref, "main");

        let spec = parse_spec("github.com/jiunbae/agent-skills").unwrap();
        assert_eq!(spec.owner, "jiunbae");
        assert_eq!(spec.path, "");
    }

    #[test]
    fn test_parse_spec_invalid() {
        assert!(parse_spec("bad-format").is_err());
    }

    #[test]
    fn remote_source_path_rejects_malformed_components() {
        for path in [
            "/absolute",
            "../outside",
            "inside/../outside",
            "./inside",
            "inside//file",
            "inside\\file",
            "inside\0file",
        ] {
            assert!(validate_source_path(path).is_err(), "accepted {path:?}");
        }
        assert!(validate_source_path("").is_ok());
        assert!(validate_source_path("personas/security-reviewer").is_ok());
    }

    #[test]
    fn extracted_path_selection_stays_within_regular_tree() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("archive-root");
        let persona = root.join("personas/reviewer");
        fs::create_dir_all(&persona).unwrap();
        fs::write(persona.join("PERSONA.md"), "persona").unwrap();

        assert_eq!(
            select_extracted_path(&root, "personas/reviewer").unwrap(),
            persona
        );
        assert_eq!(select_extracted_path(&root, "").unwrap(), root);
        assert!(select_extracted_path(&root, "personas/missing").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn extracted_path_selection_rejects_symlink_escape() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("archive-root");
        let outside = temp.path().join("outside");
        fs::create_dir_all(root.join("personas")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("PERSONA.md"), "outside sentinel").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("personas/escaped")).unwrap();

        assert!(select_extracted_path(&root, "personas/escaped").is_err());
        assert_eq!(
            fs::read_to_string(outside.join("PERSONA.md")).unwrap(),
            "outside sentinel"
        );
    }

    #[test]
    fn bounded_reader_rejects_data_beyond_limit() {
        assert_eq!(
            read_bounded(Cursor::new(b"1234"), 4, "test data").unwrap(),
            b"1234"
        );
        let error = read_bounded(Cursor::new(b"12345"), 4, "test data").unwrap_err();
        assert!(error.to_string().contains("exceeds the 4 byte limit"));
    }

    #[test]
    fn decompressed_reader_allows_exact_limit_and_errors_on_overflow() {
        let mut exact = Vec::new();
        BoundedReader::new(Cursor::new(b"1234"), 4)
            .read_to_end(&mut exact)
            .unwrap();
        assert_eq!(exact, b"1234");

        let mut overflow = Vec::new();
        let error = BoundedReader::new(Cursor::new(b"12345"), 4)
            .read_to_end(&mut overflow)
            .unwrap_err();
        assert_eq!(overflow, b"1234");
        assert!(error
            .to_string()
            .contains("decompressed tarball exceeds its byte limit"));
    }

    #[test]
    fn archive_extraction_bounds_hidden_gnu_longname_metadata() {
        let long_path = format!("repo/{}", "a".repeat(4096));
        let archive = tar_with_files(&[(&long_path, b"")]);
        let temp = tempfile::TempDir::new().unwrap();
        let error = extract_archive(
            Cursor::new(&archive),
            temp.path(),
            ArchiveLimits {
                max_archive_size: 1024,
                max_entry_size: 0,
                max_extracted_size: 0,
                max_entries: 1,
            },
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("decompressed tarball exceeds its byte limit"));
        assert!(fs::read_dir(temp.path()).unwrap().next().is_none());
    }

    #[test]
    fn archive_extraction_accepts_exact_resource_limits() {
        let archive = tar_with_files(&[("repo/file", b"1234")]);
        let temp = tempfile::TempDir::new().unwrap();

        extract_archive(
            Cursor::new(&archive),
            temp.path(),
            ArchiveLimits {
                max_archive_size: archive.len() as u64,
                max_entry_size: 4,
                max_extracted_size: 4,
                max_entries: 1,
            },
        )
        .unwrap();

        assert_eq!(fs::read(temp.path().join("repo/file")).unwrap(), b"1234");
    }

    #[test]
    fn archive_extraction_enforces_per_entry_size() {
        let archive = tar_with_files(&[("repo/file", b"12345")]);
        let temp = tempfile::TempDir::new().unwrap();
        let error = extract_archive(
            Cursor::new(&archive),
            temp.path(),
            ArchiveLimits {
                max_archive_size: archive.len() as u64,
                max_entry_size: 4,
                max_extracted_size: 10,
                max_entries: 1,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("entry exceeds the 4 byte limit"));
    }

    #[test]
    fn archive_extraction_enforces_aggregate_size() {
        let archive = tar_with_files(&[("repo/one", b"123"), ("repo/two", b"456")]);
        let temp = tempfile::TempDir::new().unwrap();
        let error = extract_archive(
            Cursor::new(&archive),
            temp.path(),
            ArchiveLimits {
                max_archive_size: archive.len() as u64,
                max_entry_size: 3,
                max_extracted_size: 5,
                max_entries: 2,
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("exceeds the 5 byte extracted-size limit"));
    }

    #[test]
    fn archive_extraction_enforces_entry_count() {
        let archive = tar_with_files(&[("repo/one", b""), ("repo/two", b"")]);
        let temp = tempfile::TempDir::new().unwrap();
        let error = extract_archive(
            Cursor::new(&archive),
            temp.path(),
            ArchiveLimits {
                max_archive_size: archive.len() as u64,
                max_entry_size: 0,
                max_extracted_size: 0,
                max_entries: 1,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("exceeds the 1 entry limit"));
    }

    #[test]
    fn archive_extraction_rejects_non_file_and_directory_entries() {
        let mut archive = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name("outside").unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "repo/link", std::io::empty())
                .unwrap();
            builder.finish().unwrap();
        }
        let temp = tempfile::TempDir::new().unwrap();
        let error = extract_archive(
            Cursor::new(&archive),
            temp.path(),
            ArchiveLimits {
                max_archive_size: archive.len() as u64,
                max_entry_size: 0,
                max_extracted_size: 0,
                max_entries: 1,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported entry type"));
        assert!(!temp.path().join("repo/link").exists());
    }

    #[test]
    fn test_parse_metadata_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skill_dir = tmp.path().join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let spec = RemoteSpec {
            owner: "jiunbae".to_string(),
            repo: "agent-skills".to_string(),
            path: "agents/background-reviewer".to_string(),
            git_ref: "v2026.02.19.1".to_string(),
        };
        write_metadata(&skill_dir, &spec).unwrap();

        let parsed = parse_metadata(&skill_dir).unwrap();
        assert_eq!(parsed.owner, "jiunbae");
        assert_eq!(parsed.repo, "agent-skills");
        assert_eq!(parsed.path, "agents/background-reviewer");
        assert_eq!(parsed.git_ref, "v2026.02.19.1");
    }

    #[test]
    fn test_parse_metadata_missing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skill_dir = tmp.path().join("no-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        assert!(parse_metadata(&skill_dir).is_err());
    }
}

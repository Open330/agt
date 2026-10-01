//! Thin wrapper over the GitHub CLI. All GitHub access for `agt sync` goes
//! through `gh`: `gh skill install` fetches skill content, `gh api` resolves
//! refs and git trees. Output parsing for the preview `gh skill` commands is
//! kept in this module so flag or format changes have one place to land.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// One entry of a recursive git tree listing.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub mode: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub sha: String,
}

impl TreeEntry {
    pub fn is_executable(&self) -> bool {
        self.kind == "blob" && self.mode == "100755"
    }
}

pub trait GhClient {
    /// Resolve a branch, tag, or SHA to a commit SHA. `None` follows gh's
    /// default: the latest release tag, else the default branch.
    fn resolve_commit(&self, repo: &str, rev: Option<&str>) -> Result<(String, String)>;
    /// Recursive tree listing (paths relative to `tree`).
    fn tree(&self, repo: &str, tree: &str) -> Result<Vec<TreeEntry>>;
    /// Install the skill at `path` pinned to `commit` into `dir/<last segment>`.
    fn install_skill(&self, repo: &str, path: &str, commit: &str, dir: &Path) -> Result<()>;
}

pub struct Gh;

impl Gh {
    pub fn new() -> Result<Self> {
        let status = Command::new("gh")
            .args(["skill", "--help"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => Ok(Self),
            Ok(_) => {
                bail!("`gh skill` is not available. Upgrade the GitHub CLI: https://cli.github.com")
            }
            Err(_) => bail!(
                "GitHub CLI (`gh`) not found. agt uses it to fetch remote skills.\n  \
                 Install: https://cli.github.com, then run `gh auth login`"
            ),
        }
    }

    fn api(&self, endpoint: &str, jq: Option<&str>) -> Result<String> {
        let mut cmd = Command::new("gh");
        cmd.arg("api").arg(endpoint);
        if let Some(jq) = jq {
            cmd.args(["--jq", jq]);
        }
        let output = cmd
            .output()
            .with_context(|| format!("Failed to run gh api {endpoint}"))?;
        if !output.status.success() {
            bail!(
                "gh api {} failed: {}",
                endpoint,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl GhClient for Gh {
    fn resolve_commit(&self, repo: &str, rev: Option<&str>) -> Result<(String, String)> {
        let rev = match rev {
            Some(rev) => rev.to_string(),
            None => self
                .api(&format!("repos/{repo}/releases/latest"), Some(".tag_name"))
                .or_else(|_| self.api(&format!("repos/{repo}"), Some(".default_branch")))?,
        };
        let sha = self.api(&format!("repos/{repo}/commits/{rev}"), Some(".sha"))?;
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("Unexpected commit SHA for {repo}@{rev}: {sha}");
        }
        Ok((rev, sha))
    }

    fn tree(&self, repo: &str, tree: &str) -> Result<Vec<TreeEntry>> {
        #[derive(Deserialize)]
        struct TreeResponse {
            tree: Vec<TreeEntry>,
            truncated: bool,
        }
        let body = self.api(&format!("repos/{repo}/git/trees/{tree}?recursive=1"), None)?;
        let response: TreeResponse =
            serde_json::from_str(&body).context("Invalid git tree response from gh api")?;
        if response.truncated {
            bail!("Git tree listing for {repo} is truncated; declare an explicit `path`");
        }
        Ok(response.tree)
    }

    fn install_skill(&self, repo: &str, path: &str, commit: &str, dir: &Path) -> Result<()> {
        let output = Command::new("gh")
            .args(["skill", "install", repo])
            .arg(format!("{path}/SKILL.md"))
            .args(["--pin", commit, "--force", "--dir"])
            .arg(dir)
            .stdin(std::process::Stdio::null())
            .output()
            .context("Failed to run gh skill install")?;
        if !output.status.success() {
            bail!(
                "gh skill install {} {} failed: {}",
                repo,
                path,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

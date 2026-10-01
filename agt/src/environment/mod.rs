//! Declarative agent environments: `agt.toml` + `agt.lock` -> installed skills.
//! Design: docs/design/0001-agent-env-manifest-and-doctor.md

pub mod integrity;
pub mod lock;
pub mod manifest;
pub mod review;

use crate::config::SkillAgent;
use crate::gh::{GhClient, TreeEntry};
use anyhow::{bail, Context, Result};
use integrity::MANAGED_MARKER;
use lock::{LockedPackage, Lockfile};
use manifest::{EnvManifest, ResolvedDep, Scope};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Env<'a> {
    pub scope: Scope,
    gh: &'a dyn GhClient,
    cache: PathBuf,
    targets: BTreeMap<SkillAgent, PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    Replace,
    Remove,
}

#[derive(Debug, Clone)]
pub struct Op {
    pub action: Action,
    pub agent: SkillAgent,
    pub name: String,
    pub dest: PathBuf,
}

#[derive(Debug, Default)]
pub struct Plan {
    /// Skills whose lock entry must be (re)resolved, or pruned lock entries.
    pub stale: Vec<String>,
    pub ops: Vec<Op>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.stale.is_empty() && self.ops.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct SyncOptions {
    /// Report differences only; never touch the network or the filesystem.
    pub check: bool,
    /// Fail instead of re-resolving when the lock does not match the manifest.
    pub frozen: bool,
    /// Re-resolve these skills even if the lock is current (`agt lock --update`).
    pub refresh: BTreeSet<String>,
}

impl<'a> Env<'a> {
    pub fn new(scope: Scope, gh: &'a dyn GhClient) -> Result<Self> {
        let cache = dirs::cache_dir()
            .context("Cannot determine cache directory")?
            .join("agt/pkgs");
        let targets = [SkillAgent::Claude, SkillAgent::Codex]
            .into_iter()
            .map(|agent| (agent, crate::config::skill_target(scope.global, agent)))
            .collect();
        Ok(Self {
            scope,
            gh,
            cache,
            targets,
        })
    }

    #[cfg(test)]
    fn with_dirs(
        scope: Scope,
        gh: &'a dyn GhClient,
        cache: PathBuf,
        targets: BTreeMap<SkillAgent, PathBuf>,
    ) -> Self {
        Self {
            scope,
            gh,
            cache,
            targets,
        }
    }

    pub fn load_manifest(&self) -> Result<EnvManifest> {
        let path = self.scope.manifest_path();
        EnvManifest::load(&path)?.with_context(|| {
            format!(
                "No {} found. Create one with `agt init{}`",
                path.display(),
                self.scope.flag()
            )
        })
    }

    pub fn load_lock(&self) -> Result<Lockfile> {
        Ok(Lockfile::load(&self.scope.lock_path())?.unwrap_or_else(Lockfile::new))
    }

    /// Bring the lock in line with the manifest, resolving only what changed.
    /// Returns the names that were (re)resolved or pruned.
    pub fn relock(
        &self,
        deps: &[ResolvedDep],
        lock: &mut Lockfile,
        refresh: &BTreeSet<String>,
    ) -> Result<Vec<String>> {
        let stale = stale_entries(deps, lock, refresh);
        if stale.is_empty() {
            return Ok(stale);
        }
        let wanted: BTreeSet<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        lock.packages
            .retain(|p| p.kind != "skill" || wanted.contains(p.name.as_str()));
        for dep in deps.iter().filter(|d| stale.contains(&d.name)) {
            let pkg = self.resolve(dep)?;
            lock.packages
                .retain(|p| !(p.kind == "skill" && p.name == dep.name));
            lock.packages.push(pkg);
        }
        Ok(stale)
    }

    fn resolve(&self, dep: &ResolvedDep) -> Result<LockedPackage> {
        crate::ui::info(&format!("Resolving {} from {}", dep.name, dep.repo));
        let (rev, commit) = self.gh.resolve_commit(&dep.repo, dep.rev.as_deref())?;
        let entries = self.gh.tree(&dep.repo, &commit)?;
        let path = match &dep.path {
            Some(path) => path.clone(),
            None => find_skill_path(&entries, &dep.name)
                .with_context(|| format!("Cannot locate skill '{}' in {}", dep.name, dep.repo))?,
        };
        let tree = entries
            .iter()
            .find(|e| e.kind == "tree" && e.path == path)
            .with_context(|| format!("{}@{} has no directory {}", dep.repo, rev, path))?;
        if !entries
            .iter()
            .any(|e| e.kind == "blob" && e.path == format!("{path}/SKILL.md"))
        {
            bail!("{}@{}: {} has no SKILL.md", dep.repo, rev, path);
        }
        let prefix = format!("{path}/");
        let executables = entries
            .iter()
            .filter(|e| e.is_executable())
            .filter_map(|e| e.path.strip_prefix(&prefix).map(str::to_string))
            .collect();
        let mut pkg = LockedPackage {
            kind: "skill".into(),
            name: dep.name.clone(),
            source: format!("github:{}", dep.repo),
            rev,
            commit,
            path,
            tree: tree.sha.clone(),
            integrity: String::new(),
            executables,
        };
        self.fetch(&mut pkg)?;
        Ok(pkg)
    }

    /// Materialize a package in the cache and return its directory. Fills in
    /// `integrity` on first fetch; afterwards a mismatch is a hard error.
    fn fetch(&self, pkg: &mut LockedPackage) -> Result<PathBuf> {
        let leaf = pkg.path.rsplit('/').next().unwrap_or(&pkg.path).to_string();
        let cached = self.cache.join(&pkg.tree).join(&leaf);
        if cached.join("SKILL.md").is_file() && integrity::hash_dir(&cached)? == pkg.integrity {
            return Ok(cached);
        }

        fs::create_dir_all(&self.cache)?;
        let staging = tempfile::Builder::new()
            .prefix(".fetch-")
            .tempdir_in(&self.cache)?;
        self.gh
            .install_skill(pkg.repo(), &pkg.path, &pkg.commit, staging.path())?;
        let installed = staging.path().join(&leaf);
        let skill_md = fs::read_to_string(installed.join("SKILL.md"))
            .with_context(|| format!("gh skill install did not produce {}/SKILL.md", leaf))?;

        let tree = gh_tree_sha(&skill_md);
        if tree.as_deref() != Some(pkg.tree.as_str()) {
            bail!(
                "{}: installed tree {} does not match locked tree {}",
                pkg.name,
                tree.unwrap_or_else(|| "<missing>".into()),
                pkg.tree
            );
        }
        // gh skill install drops the executable bit; restore it from git modes.
        restore_exec_bits(&installed, &pkg.executables)?;

        let hash = integrity::hash_dir(&installed)?;
        if pkg.integrity.is_empty() {
            pkg.integrity = hash;
        } else if hash != pkg.integrity {
            bail!(
                "{}: content hash {} does not match agt.lock ({}). \
                 The upstream commit may have been rewritten; run `agt lock --update {}` to accept it.",
                pkg.name,
                hash,
                pkg.integrity,
                pkg.name
            );
        }

        if cached.exists() {
            fs::remove_dir_all(&cached)?;
        }
        fs::create_dir_all(cached.parent().expect("cache entry has parent"))?;
        fs::rename(&installed, &cached)?;
        Ok(cached)
    }

    pub fn plan(&self, deps: &[ResolvedDep], lock: &Lockfile) -> Result<Vec<Op>> {
        let owner = self.scope.owner();
        let mut ops = Vec::new();
        for (&agent, root) in &self.targets {
            let desired: BTreeMap<&str, &LockedPackage> = deps
                .iter()
                .filter(|d| d.agents.contains(&agent))
                .filter_map(|d| lock.skill(&d.name).map(|p| (d.name.as_str(), p)))
                .collect();

            for (name, pkg) in &desired {
                let dest = root.join(name);
                let action = if !dest.exists() && !dest.is_symlink() {
                    Some(Action::Install)
                } else {
                    match Marker::read(&dest) {
                        Some(marker) if marker.owner == owner => {
                            let current = integrity::hash_dir(&dest).ok();
                            (current.as_deref() != Some(pkg.integrity.as_str()))
                                .then_some(Action::Replace)
                        }
                        Some(marker) => bail!(
                            "{} is managed by another manifest ({})",
                            dest.display(),
                            marker.owner
                        ),
                        None => bail!(
                            "{} already exists and is not managed by agt. \
                             Remove it or pick another name in agt.toml.",
                            dest.display()
                        ),
                    }
                };
                if let Some(action) = action {
                    ops.push(Op {
                        action,
                        agent,
                        name: name.to_string(),
                        dest,
                    });
                }
            }

            for dest in managed_dirs(root, &owner) {
                let name = dest.file_name().unwrap().to_string_lossy().into_owned();
                if !desired.contains_key(name.as_str()) {
                    ops.push(Op {
                        action: Action::Remove,
                        agent,
                        name,
                        dest,
                    });
                }
            }
        }
        Ok(ops)
    }

    /// Take ownership of existing unmanaged copies of `name`, which the caller
    /// has just declared in agt.toml. Copies identical to the locked content
    /// are marked in place; others are moved to a backup so `sync` can install
    /// the locked version. Returns `(agent, backup)` per adopted copy.
    pub fn adopt(&self, name: &str) -> Result<Vec<(SkillAgent, Option<PathBuf>)>> {
        let deps = self.load_manifest()?.deps()?;
        let mut lock = self.load_lock()?;
        self.relock(&deps, &mut lock, &BTreeSet::new())?;
        let pkg = lock
            .skill(name)
            .with_context(|| format!("'{name}' is not declared in agt.toml"))?;

        let mut adopted = Vec::new();
        for (&agent, root) in &self.targets {
            let dir = root.join(name);
            if dir.is_symlink() || !dir.is_dir() || Marker::read(&dir).is_some() {
                continue;
            }
            // gh-installed copies differ from the lock only by dropped exec bits.
            restore_exec_bits(&dir, &pkg.executables)?;
            if integrity::hash_dir(&dir)? == pkg.integrity {
                Marker {
                    owner: self.scope.owner(),
                    name: name.to_string(),
                    integrity: pkg.integrity.clone(),
                }
                .write(&dir)?;
                adopted.push((agent, None));
            } else {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or_default();
                let backup = self
                    .cache
                    .with_file_name("adopted")
                    .join(format!("{name}-{agent}-{stamp}"));
                fs::create_dir_all(backup.parent().expect("backup has parent"))?;
                fs::rename(&dir, &backup)
                    .with_context(|| format!("Failed to move {} aside", dir.display()))?;
                adopted.push((agent, Some(backup)));
            }
        }
        if adopted.is_empty() {
            bail!("No unmanaged '{name}' found in this scope's skill directories");
        }
        lock.save(&self.scope.lock_path())?;
        Ok(adopted)
    }

    /// Compare each locked skill with what its declared rev points at now.
    pub fn outdated(&self) -> Result<Vec<Outdated>> {
        let deps = self.load_manifest()?.deps()?;
        let lock = self.load_lock()?;
        let mut out = Vec::new();
        for dep in &deps {
            let Some(pkg) = lock.skill(&dep.name) else {
                out.push(Outdated::new(dep, None, Status::NotLocked));
                continue;
            };
            if dep.rev.as_deref().is_some_and(is_commit_sha) {
                out.push(Outdated::new(dep, Some(pkg), Status::Pinned));
                continue;
            }
            let (rev, commit) = self.gh.resolve_commit(&dep.repo, dep.rev.as_deref())?;
            let status = if commit == pkg.commit {
                Status::Current
            } else {
                let entries = self.gh.tree(&dep.repo, &commit)?;
                match entries
                    .iter()
                    .find(|e| e.kind == "tree" && e.path == pkg.path)
                {
                    None => Status::Missing { rev, commit },
                    Some(tree) if tree.sha == pkg.tree => Status::CommitOnly { rev, commit },
                    Some(_) => Status::Changed { rev, commit },
                }
            };
            out.push(Outdated::new(dep, Some(pkg), status));
        }
        Ok(out)
    }

    /// Resolve `names` (all when empty) to their latest commit and review what
    /// changed. Nothing is written; pass approved packages to `apply_updates`.
    pub fn plan_updates(&self, names: &BTreeSet<String>) -> Result<Vec<Update>> {
        let deps = self.load_manifest()?.deps()?;
        for name in names {
            if !deps.iter().any(|d| &d.name == name) {
                bail!("'{name}' is not declared in agt.toml");
            }
        }
        let lock = self.load_lock()?;
        let mut updates = Vec::new();
        for dep in deps
            .iter()
            .filter(|d| names.is_empty() || names.contains(&d.name))
        {
            let Some(old) = lock.skill(&dep.name).cloned() else {
                bail!(
                    "'{}' is not locked yet; run `agt lock{}` first",
                    dep.name,
                    self.scope.flag()
                );
            };
            let mut new = self.resolve(dep)?;
            if new.commit == old.commit {
                continue;
            }
            let review = if new.tree == old.tree {
                None
            } else {
                let old_dir = self.fetch(&mut old.clone())?;
                let new_dir = self.fetch(&mut new)?;
                Some(review::review(&old_dir, &new_dir))
            };
            updates.push(Update { old, new, review });
        }
        Ok(updates)
    }

    /// Write approved packages into agt.lock and install them.
    pub fn apply_updates(&self, approved: Vec<LockedPackage>) -> Result<Plan> {
        let mut lock = self.load_lock()?;
        for pkg in approved {
            lock.packages
                .retain(|p| !(p.kind == pkg.kind && p.name == pkg.name));
            lock.packages.push(pkg);
        }
        lock.save(&self.scope.lock_path())?;
        self.sync(&SyncOptions::default())
    }

    pub fn sync(&self, opts: &SyncOptions) -> Result<Plan> {
        let manifest = self.load_manifest()?;
        let deps = manifest.deps()?;
        let mut lock = self.load_lock()?;

        if opts.check || opts.frozen {
            let stale = stale_entries(&deps, &lock, &opts.refresh);
            if !stale.is_empty() {
                if opts.frozen {
                    bail!(
                        "agt.lock is out of date for: {}. Run `agt lock{}` and commit the result.",
                        stale.join(", "),
                        self.scope.flag()
                    );
                }
                let ops = self.plan(&deps, &lock)?;
                return Ok(Plan { stale, ops });
            }
        }

        let stale = if opts.check {
            Vec::new()
        } else {
            self.relock(&deps, &mut lock, &opts.refresh)?
        };
        let ops = self.plan(&deps, &lock)?;
        if opts.check {
            return Ok(Plan { stale, ops });
        }

        for op in &ops {
            match op.action {
                Action::Install | Action::Replace => {
                    let pkg = lock
                        .packages
                        .iter_mut()
                        .find(|p| p.kind == "skill" && p.name == op.name)
                        .expect("planned skill is locked");
                    let src = self.fetch(pkg)?;
                    let marker = Marker {
                        owner: self.scope.owner(),
                        name: op.name.clone(),
                        integrity: pkg.integrity.clone(),
                    };
                    fs::create_dir_all(op.dest.parent().expect("skill dest has parent"))?;
                    crate::util::replace_dir_transactionally(&src, &op.dest, |staged| {
                        marker.write(staged)
                    })
                    .with_context(|| format!("Failed to install {}", op.name))?;
                }
                Action::Remove => {
                    fs::remove_dir_all(&op.dest)
                        .with_context(|| format!("Failed to remove {}", op.dest.display()))?;
                }
            }
        }

        if !stale.is_empty() || !self.scope.lock_path().exists() {
            lock.save(&self.scope.lock_path())?;
        }
        Ok(Plan { stale, ops })
    }
}

fn is_commit_sha(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Status {
    Current,
    /// The declared rev moved, but this skill's tree did not change.
    CommitOnly {
        rev: String,
        commit: String,
    },
    Changed {
        rev: String,
        commit: String,
    },
    /// The skill path no longer exists at the declared rev.
    Missing {
        rev: String,
        commit: String,
    },
    /// Declared with a commit SHA; `update` never moves it.
    Pinned,
    NotLocked,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Outdated {
    pub name: String,
    pub repo: String,
    pub locked: Option<String>,
    #[serde(flatten)]
    pub status: Status,
}

impl Outdated {
    fn new(dep: &ResolvedDep, pkg: Option<&LockedPackage>, status: Status) -> Self {
        Self {
            name: dep.name.clone(),
            repo: dep.repo.clone(),
            locked: pkg.map(|p| p.commit.clone()),
            status,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Update {
    pub old: LockedPackage,
    pub new: LockedPackage,
    /// `None` when only the commit moved and the skill's content is identical.
    pub review: Option<review::Review>,
}

fn restore_exec_bits(dir: &Path, executables: &[String]) -> Result<()> {
    for rel in executables {
        let file = dir.join(rel);
        if file.is_file() && !file.is_symlink() {
            let mut perms = fs::metadata(&file)?.permissions();
            perms.set_mode(perms.mode() | 0o111);
            fs::set_permissions(&file, perms)?;
        }
    }
    Ok(())
}

pub(crate) fn stale_entries(
    deps: &[ResolvedDep],
    lock: &Lockfile,
    refresh: &BTreeSet<String>,
) -> Vec<String> {
    let mut stale: Vec<String> = deps
        .iter()
        .filter(|dep| {
            refresh.contains(&dep.name)
                || match lock.skill(&dep.name) {
                    None => true,
                    Some(pkg) => {
                        pkg.repo() != dep.repo
                            || dep.rev.as_ref().is_some_and(|rev| *rev != pkg.rev)
                            || dep.path.as_ref().is_some_and(|path| *path != pkg.path)
                            || pkg.integrity.is_empty()
                    }
                }
        })
        .map(|dep| dep.name.clone())
        .collect();
    let wanted: BTreeSet<&str> = deps.iter().map(|d| d.name.as_str()).collect();
    stale.extend(
        lock.packages
            .iter()
            .filter(|p| p.kind == "skill" && !wanted.contains(p.name.as_str()))
            .map(|p| p.name.clone()),
    );
    stale
}

/// Find the unique directory named `name` that holds a SKILL.md.
fn find_skill_path(entries: &[TreeEntry], name: &str) -> Result<String> {
    let candidates: Vec<&str> = entries
        .iter()
        .filter(|e| e.kind == "blob")
        .filter_map(|e| e.path.strip_suffix("/SKILL.md"))
        .filter(|dir| dir.rsplit('/').next() == Some(name))
        .collect();
    match candidates.as_slice() {
        [one] => Ok(one.to_string()),
        [] => bail!("no directory named '{name}' with a SKILL.md"),
        many => bail!(
            "'{name}' is ambiguous: {}. Set `path` explicitly.",
            many.join(", ")
        ),
    }
}

fn gh_tree_sha(skill_md: &str) -> Option<String> {
    let rest = skill_md.trim_start().strip_prefix("---")?;
    let yaml = &rest[..rest.find("\n---")?];
    let fm: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    fm.get("metadata")?
        .get("github-tree-sha")?
        .as_str()
        .map(str::to_string)
}

fn managed_dirs(root: &Path, owner: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| !p.is_symlink() && p.is_dir())
        .filter(|p| Marker::read(p).is_some_and(|m| m.owner == owner))
        .collect();
    dirs.sort();
    dirs
}

/// `.agt-managed`: ownership record written into each installed skill.
#[derive(Debug, PartialEq, Eq)]
pub struct Marker {
    /// `project` for the repository's own agt.toml, else the user manifest path.
    pub owner: String,
    pub name: String,
    pub integrity: String,
}

impl Marker {
    pub fn read(dir: &Path) -> Option<Self> {
        let content = fs::read_to_string(dir.join(MANAGED_MARKER)).ok()?;
        let field = |key: &str| {
            content
                .lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
                .map(|v| v.trim().to_string())
        };
        Some(Self {
            owner: field("owner")?,
            name: field("name")?,
            integrity: field("integrity").unwrap_or_default(),
        })
    }

    pub(crate) fn write(&self, dir: &Path) -> Result<()> {
        fs::write(
            dir.join(MANAGED_MARKER),
            format!(
                "owner: {}\nname: {}\nintegrity: {}\n",
                self.owner, self.name, self.integrity
            ),
        )
        .context("Failed to write .agt-managed")
    }
}

#[cfg(test)]
mod tests;

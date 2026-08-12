use crate::{config, frontmatter, remote, ui, util};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum SkillAction {
    /// Install a skill (local symlink or remote)
    Install {
        /// Skill name (from source library)
        name: Option<String>,
        /// Install globally in the selected agent's user skill directory
        #[arg(short, long)]
        global: bool,
        /// Agent whose skill directory should receive the installation
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
        /// Force overwrite existing
        #[arg(short, long)]
        force: bool,
        /// Install a named profile (core, dev, all, etc.)
        #[arg(short, long, value_name = "NAME")]
        profile: Option<String>,
        /// Install all available skills
        #[arg(short, long)]
        all: bool,
        /// Remote spec: owner/repo/path[@ref]
        #[arg(long, value_name = "SPEC")]
        from: Option<String>,
    },
    /// Uninstall a skill
    Uninstall {
        /// Skill name
        name: String,
        /// Remove from global scope
        #[arg(short, long)]
        global: bool,
        /// Agent whose skill directory should be modified
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
    },
    /// List available and installed skills
    List {
        /// Show only installed skills
        #[arg(long)]
        installed: bool,
        /// Show only local project skills
        #[arg(long)]
        local: bool,
        /// Show only global skills
        #[arg(long)]
        global: bool,
        /// Show available installation profiles
        #[arg(long)]
        profiles: bool,
        /// Agent whose installed skills should be listed
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Initialize skill directory in current project
    Init {
        /// Agent whose project skill directory should be created
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
    },
    /// Show the path of a skill
    Which {
        /// Skill name
        name: String,
        /// Agent whose installed skills should be searched
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
    },
    /// Update remote-installed skills
    Update {
        /// Skill or group name (omit to update all remote skills)
        name: Option<String>,
        /// Update only global skills
        #[arg(short, long)]
        global: bool,
        /// Update only local skills
        #[arg(short, long)]
        local: bool,
        /// Agent whose remote-installed skills should be updated
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
    },
    /// Run a prompt with an optional skill (omit skill to call LLM directly)
    #[command(alias = "run")]
    Use {
        /// Skill name (optional — omit to call LLM directly)
        #[arg(long, short)]
        skill: Option<String>,
        /// LLM to use: claude, codex, opencode, gemini, ollama
        #[arg(long)]
        llm: Option<String>,
        /// The prompt to execute
        prompt: Vec<String>,
    },
}

pub fn execute(action: SkillAction) -> Result<()> {
    match action {
        SkillAction::Install {
            name,
            global,
            agent,
            force,
            profile,
            all,
            from,
        } => install(name, global, agent, force, profile, all, from),
        SkillAction::Uninstall {
            name,
            global,
            agent,
        } => uninstall(&name, global, agent),
        SkillAction::List {
            installed,
            local,
            global,
            profiles,
            agent,
            json,
        } => list(installed, local, global, profiles, agent, json),
        SkillAction::Init { agent } => init(agent),
        SkillAction::Which { name, agent } => which(&name, agent),
        SkillAction::Update {
            name,
            global,
            local,
            agent,
        } => update(name, global, local, agent),
        SkillAction::Use { skill, llm, prompt } => {
            let prompt_str = prompt.join(" ");
            if prompt_str.trim().is_empty() {
                bail!("No prompt provided. Usage: agt skill use \"your prompt\" [-s skill_name]");
            }
            super::run::execute(&prompt_str, skill.as_deref(), llm.as_deref())
        }
    }
}

fn install(
    name: Option<String>,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    profile: Option<String>,
    all: bool,
    from: Option<String>,
) -> Result<()> {
    // Profile / all install
    let profile_name = if all {
        if profile.is_some() {
            bail!("--all and --profile cannot be used together");
        }
        Some("all".to_string())
    } else {
        profile
    };

    if let Some(spec_str) = from {
        if name.is_some() && profile_name.is_some() {
            bail!("Cannot specify both a skill name and --profile/--all");
        }
        return install_remote(
            &spec_str,
            global,
            agent,
            force,
            profile_name.as_deref(),
            name.as_deref(),
        );
    }

    if let Some(prof_name) = profile_name {
        if name.is_some() {
            bail!("Cannot specify both a skill name and --profile/--all");
        }
        return install_profile(&prof_name, global, agent, force);
    }

    let name = match name {
        Some(n) => n,
        None => {
            if !console::Term::stderr().is_term() {
                bail!("Skill name required (or use --profile, --all, --from)");
            }
            return interactive_install(global, agent, force);
        }
    };
    util::validate_name(&name)?;

    let source_dir = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .context(config::source_dir_hint())?;

    // Find skill in source
    let skill_path = find_skill_in_source(&source_dir, &name)
        .context(format!("Skill '{}' not found in source library", name))?;

    // Extract group from skill_path (parent of skill dir)
    let group = skill_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|g| g.to_string_lossy().to_string())
        .unwrap_or_default();

    let target_dir = config::skill_target(global, agent);

    // Check cross-scope duplicate
    if !force {
        let local_dir = config::skill_target(false, agent);
        let global_dir = config::skill_target(true, agent);
        if warn_cross_scope_duplicate(&name, &group, global, &local_dir, &global_dir) {
            return Ok(());
        }
    }

    let link_path = config::skill_destination(&target_dir, &group, &name, agent);
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }

    install_single_local_skill_link(&skill_path, &link_path, force, &name)?;

    let scope = if global { "global" } else { "local" };
    ui::success(&format!(
        "Installed skill '{}/{}' ({}, {})",
        group, name, scope, agent
    ));
    Ok(())
}

fn install_single_local_skill_link(
    skill_path: &Path,
    link_path: &Path,
    force: bool,
    display_name: &str,
) -> Result<()> {
    install_single_local_skill_link_with(
        skill_path,
        link_path,
        force,
        display_name,
        |source, destination| util::replace_symlink_transactionally(source, destination),
    )
}

fn install_single_local_skill_link_with<R>(
    skill_path: &Path,
    link_path: &Path,
    force: bool,
    display_name: &str,
    mut replace: R,
) -> Result<()>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    if !force {
        util::ensure_target_clear(link_path, false, display_name)?;
    }
    create_local_skill_link_with(skill_path, link_path, force, display_name, &mut replace)
}

fn create_local_skill_link_with<R>(
    skill_path: &Path,
    link_path: &Path,
    force: bool,
    display_name: &str,
    replace: &mut R,
) -> Result<()>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    if force {
        replace(skill_path, link_path)
    } else {
        symlink(skill_path, link_path).map_err(anyhow::Error::from)
    }
    .with_context(|| {
        format!(
            "Failed to create symlink for '{}': {} -> {}",
            display_name,
            link_path.display(),
            skill_path.display()
        )
    })
}

fn install_remote(
    spec_str: &str,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    profile: Option<&str>,
    requested_name: Option<&str>,
) -> Result<()> {
    let spec = remote::parse_spec(spec_str)?;
    validate_remote_source_path(&spec)?;

    // Repo-level: owner/repo with no path — browse all skills
    if spec.path.is_empty() {
        return install_remote_repo(&spec, global, agent, force, profile, requested_name);
    }
    if profile.is_some() {
        bail!("--profile/--all requires a repository-level --from spec");
    }
    if requested_name.is_some() {
        bail!("A skill name cannot be combined with a path-level --from spec");
    }

    ui::info(&format!("Downloading {}...", spec));

    let (tmp_dir, source_path) = remote::fetch_dir(&spec)?;

    // Verify it's a skill (has SKILL.md)
    if !source_path.join("SKILL.md").exists() {
        bail!("Remote path does not contain SKILL.md: {}", spec);
    }
    validate_fetched_skill_path(tmp_dir.path(), &source_path)?;

    let skill_name = source_path
        .file_name()
        .context("Invalid remote path")?
        .to_string_lossy()
        .to_string();
    util::validate_name(&skill_name)?;

    let target_dir = config::skill_target(global, agent);

    let group = remote_skill_group(&spec.path);
    validate_skill_pair(&group, &skill_name)?;
    let dest = config::skill_destination(&target_dir, &group, &skill_name, agent);
    validate_skill_destination(&target_dir, &group, &dest, agent)?;
    if !force {
        util::ensure_target_clear(&dest, false, &skill_name)?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    install_remote_skill_from_source(&source_path, &dest, &spec, force)?;

    let scope = if global { "global" } else { "local" };
    let installed_name = if group.is_empty() {
        skill_name.clone()
    } else {
        format!("{group}/{skill_name}")
    };
    ui::success(&format!(
        "Installed remote skill '{}' ({}, {}) from {}",
        installed_name, scope, agent, spec
    ));
    Ok(())
}

fn remote_skill_group(path: &str) -> String {
    Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn validate_skill_pair(group: &str, skill_name: &str) -> Result<()> {
    if !group.is_empty() {
        util::validate_name(group).context("Invalid skill group")?;
    }
    util::validate_name(skill_name).context("Invalid skill name")?;
    Ok(())
}

fn validate_remote_source_path(spec: &remote::RemoteSpec) -> Result<()> {
    if spec.path.contains('\\') || Path::new(&spec.path).is_absolute() {
        bail!("Remote source path must be relative: {}", spec.path);
    }
    for component in Path::new(&spec.path).components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            bail!("Remote source path contains traversal: {}", spec.path);
        }
    }
    Ok(())
}

fn validate_fetched_skill_path(download_root: &Path, source_path: &Path) -> Result<()> {
    if download_root.is_symlink() {
        bail!(
            "Remote download root is a symlink: {}",
            download_root.display()
        );
    }
    let relative = source_path.strip_prefix(download_root).with_context(|| {
        format!(
            "Remote skill path escapes download root: {}",
            source_path.display()
        )
    })?;
    let mut current = download_root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            bail!("Remote skill path is malformed: {}", source_path.display());
        };
        current.push(component);
        if current.is_symlink() {
            bail!(
                "Remote skill path traverses a symlink: {}",
                current.display()
            );
        }
    }
    let root = fs::canonicalize(download_root)?;
    let source = fs::canonicalize(source_path)?;
    if !source.starts_with(&root) || !source.is_dir() {
        bail!(
            "Remote skill path escapes download root: {}",
            source_path.display()
        );
    }
    let manifest = source_path.join("SKILL.md");
    if manifest.is_symlink() || !manifest.is_file() {
        bail!(
            "Remote skill manifest is not a regular file: {}",
            manifest.display()
        );
    }
    Ok(())
}

fn validate_skill_destination(
    target_dir: &Path,
    group: &str,
    destination: &Path,
    agent: config::SkillAgent,
) -> Result<()> {
    if target_dir.is_symlink() {
        bail!("Skill root is a symlink: {}", target_dir.display());
    }
    if !destination.starts_with(target_dir) || destination == target_dir {
        bail!(
            "Skill destination escapes selected root: {}",
            destination.display()
        );
    }
    if agent == config::SkillAgent::Claude && !group.is_empty() {
        let group_dir = target_dir.join(group);
        if group_dir.is_symlink() {
            bail!(
                "Skill destination traverses a symlink: {}",
                group_dir.display()
            );
        }
    }
    Ok(())
}

fn install_remote_skill_from_source(
    source: &Path,
    destination: &Path,
    spec: &remote::RemoteSpec,
    force: bool,
) -> Result<()> {
    install_remote_skill_from_source_with(source, destination, force, |staged| {
        remote::write_metadata(staged, spec)
    })
}

fn install_remote_skill_from_source_with<F>(
    source: &Path,
    destination: &Path,
    force: bool,
    prepare: F,
) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if !force && (destination.exists() || destination.is_symlink()) {
        bail!("Skill already exists at {}", destination.display());
    }
    util::replace_dir_transactionally(source, destination, |staged| {
        let manifest = staged.join("SKILL.md");
        if manifest.is_symlink() || !manifest.is_file() {
            bail!("Staged remote skill does not contain a regular SKILL.md");
        }
        prepare(staged)
    })
}

fn install_remote_repo(
    spec: &remote::RemoteSpec,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    profile: Option<&str>,
    requested_name: Option<&str>,
) -> Result<()> {
    ui::info(&format!(
        "Downloading {}/{}@{}...",
        spec.owner, spec.repo, spec.git_ref
    ));
    let (tmp_dir, repo_root) = remote::fetch_dir(spec)?;

    // Discover skills in the repo (directories containing SKILL.md)
    let groups = config::skill_groups(&repo_root);
    let mut all_skills: Vec<(String, String)> = Vec::new();
    for group in &groups {
        for skill_name in config::skills_in_group(&repo_root, group) {
            all_skills.push((group.clone(), skill_name));
        }
    }

    // Also check for personas
    let persona_dir = repo_root.join("personas");
    let has_personas = persona_dir.is_dir()
        && fs::read_dir(&persona_dir)
            .map(|rd| {
                rd.flatten()
                    .any(|e| !e.file_name().to_string_lossy().starts_with('.'))
            })
            .unwrap_or(false);

    if all_skills.is_empty() {
        bail!("No skills found in {}/{}", spec.owner, spec.repo);
    }

    ui::info(&format!(
        "Found {} skills in {} groups{}",
        all_skills.len(),
        groups.len(),
        if has_personas { " (+ personas)" } else { "" }
    ));

    // Interactive mode if TTY
    let is_tty = console::Term::stderr().is_term();

    let target_dir = config::skill_target(global, agent);

    let scope = if global { "global" } else { "local" };
    let skills_to_install = if let Some(requested_name) = requested_name {
        util::validate_name(requested_name)?;
        let matches = skills_named(&all_skills, requested_name);
        match matches.len() {
            0 => bail!(
                "Skill '{}' not found in {}/{}",
                requested_name,
                spec.owner,
                spec.repo
            ),
            1 => matches,
            _ => bail!(
                "Skill name '{}' is ambiguous in {}/{}",
                requested_name,
                spec.owner,
                spec.repo
            ),
        }
    } else if let Some(profile_name) = profile {
        config::resolve_profile(profile_name, &repo_root)?.skills
    } else if is_tty {
        let local_installed = installed_skill_names(&config::skill_target(false, agent));
        let global_installed = installed_skill_names(&config::skill_target(true, agent));

        let selection = ui::interactive::run_interactive_selector_remote(
            &repo_root,
            &local_installed,
            &global_installed,
        )?;

        match selection {
            ui::interactive::InteractiveSelection::Profile(prof_name) => {
                let resolved = config::resolve_profile(&prof_name, &repo_root)?;
                if !ui::interactive::confirm_install(&resolved.skills, global)? {
                    ui::info("Installation cancelled.");
                    return Ok(());
                }
                resolved.skills
            }
            ui::interactive::InteractiveSelection::Skills(skills) => {
                if !ui::interactive::confirm_install(&skills, global)? {
                    ui::info("Installation cancelled.");
                    return Ok(());
                }
                skills
            }
            _ => {
                ui::info("Installation cancelled.");
                return Ok(());
            }
        }
    } else {
        // Non-interactive: install all
        all_skills
    };

    validate_skill_install_plan(
        tmp_dir.path(),
        &repo_root,
        &target_dir,
        &skills_to_install,
        agent,
    )?;
    fs::create_dir_all(&target_dir)?;

    let mut installed = 0;
    let mut skipped = 0;
    let local_dir = config::skill_target(false, agent);
    let global_dir = config::skill_target(true, agent);

    for (group, skill_name) in &skills_to_install {
        let source_path = repo_root.join(group).join(skill_name);
        if !source_path.is_dir() || !source_path.join("SKILL.md").exists() {
            skipped += 1;
            continue;
        }

        // Check cross-scope duplicate
        if !force && warn_cross_scope_duplicate(skill_name, group, global, &local_dir, &global_dir)
        {
            skipped += 1;
            continue;
        }

        let dest = config::skill_destination(&target_dir, group, skill_name, agent);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        if !force && (dest.exists() || dest.is_symlink()) {
            skipped += 1;
            continue;
        }

        let skill_spec = remote::RemoteSpec {
            owner: spec.owner.clone(),
            repo: spec.repo.clone(),
            path: format!("{}/{}", group, skill_name),
            git_ref: spec.git_ref.clone(),
        };
        install_remote_skill_from_source(&source_path, &dest, &skill_spec, force)?;
        ui::success(&format!(
            "Installed skill '{}/{}' ({}, {})",
            group, skill_name, scope, agent
        ));
        installed += 1;
    }

    ui::success(&format!(
        "Done: {} installed, {} skipped from {}/{}",
        installed, skipped, spec.owner, spec.repo
    ));

    // A fetched repository is untrusted input. Its manifest must never be allowed
    // to write to the user's home directory implicitly.
    run_manifest_setup(&repo_root, ManifestSource::Remote)?;

    Ok(())
}

fn validate_skill_install_plan(
    download_root: &Path,
    source_root: &Path,
    target_root: &Path,
    skills: &[(String, String)],
    agent: config::SkillAgent,
) -> Result<()> {
    for (group, skill_name) in skills {
        validate_skill_pair(group, skill_name)?;
        let source = source_root.join(group).join(skill_name);
        validate_fetched_skill_path(download_root, &source).with_context(|| {
            format!("Invalid source for remote skill '{}/{}'", group, skill_name)
        })?;
        let destination = config::skill_destination(target_root, group, skill_name, agent);
        validate_skill_destination(target_root, group, &destination, agent).with_context(|| {
            format!(
                "Invalid destination for remote skill '{}/{}'",
                group, skill_name
            )
        })?;
    }
    Ok(())
}

fn skills_named(all_skills: &[(String, String)], requested_name: &str) -> Vec<(String, String)> {
    all_skills
        .iter()
        .filter(|(_, skill_name)| skill_name == requested_name)
        .cloned()
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManifestSource {
    TrustedLocal,
    Remote,
}

/// Execute [[setup.copy]] rules from an application-owned local source.
/// Remote repository manifests are intentionally ignored because installation is
/// not explicit consent to replace arbitrary files below the user's home directory.
fn run_manifest_setup(repo_root: &Path, source_kind: ManifestSource) -> Result<()> {
    run_manifest_setup_with(repo_root, source_kind, config::resolve_home)
}

fn run_manifest_setup_with<F>(
    repo_root: &Path,
    source_kind: ManifestSource,
    resolve_target: F,
) -> Result<()>
where
    F: Fn(&str) -> PathBuf,
{
    if source_kind == ManifestSource::Remote {
        return Ok(());
    }

    let manifest = match config::parse_manifest(repo_root)? {
        Some(m) => m,
        None => return Ok(()),
    };

    if manifest.setup.copy.is_empty() {
        return Ok(());
    }

    let mut total_copied = 0;

    for rule in &manifest.setup.copy {
        let source = repo_root.join(&rule.from);
        if !source.exists() {
            continue;
        }

        let target = resolve_target(&rule.to);

        // If target is a symlink, user manages it — skip
        if target.is_symlink() {
            ui::info(&format!("{} is a symlink, skipping", rule.to));
            continue;
        }

        let copied = if source.is_dir() {
            copy_dir_with_strategy(&source, &target, &rule.strategy)?
        } else {
            copy_file_with_strategy(&source, &target, &rule.strategy)?
        };

        total_copied += copied;
    }

    if total_copied > 0 {
        ui::success(&format!("Post-install: copied {} files", total_copied));
    }

    Ok(())
}

/// Copy a directory's contents to target using the given strategy.
/// "merge" skips existing files; "replace" overwrites everything.
fn copy_dir_with_strategy(source: &Path, target: &Path, strategy: &str) -> Result<usize> {
    fs::create_dir_all(target)?;
    let mut copied = 0;

    for entry in fs::read_dir(source)?.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }

        let dest = target.join(entry.file_name());

        if ft.is_dir() {
            copied += copy_dir_with_strategy(&entry.path(), &dest, strategy)?;
        } else {
            copied += copy_file_with_strategy(&entry.path(), &dest, strategy)?;
        }
    }

    Ok(copied)
}

/// Copy a single file to target using the given strategy.
fn copy_file_with_strategy(source: &Path, target: &Path, strategy: &str) -> Result<usize> {
    if target.exists() && strategy == "merge" {
        return Ok(0);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(source, target).context(format!(
        "Failed to copy {} -> {}",
        source.display(),
        target.display()
    ))?;

    Ok(1)
}

fn uninstall(name: &str, global: bool, agent: config::SkillAgent) -> Result<()> {
    let target_dir = config::skill_target(global, agent);
    let scope = if global { "global" } else { "local" };
    uninstall_from_target(name, &target_dir, scope, agent)
}

fn uninstall_from_target(
    selector: &str,
    target_dir: &Path,
    scope: &str,
    agent: config::SkillAgent,
) -> Result<()> {
    let name = validate_uninstall_selector(selector)?;

    // Check if name matches a real group directory (e.g. "acme/")
    let group_dir = target_dir.join(&name);
    if group_dir.is_dir() && !group_dir.join("SKILL.md").exists() {
        ensure_confined_removal(target_dir, &group_dir, true)?;
        return uninstall_group(target_dir, &group_dir, &name, scope);
    }

    // Check if name matches a virtual group (e.g. "other" — flat skills with inferred group)
    let virtual_skills = find_virtual_group_skills(target_dir, &name);
    if !virtual_skills.is_empty() {
        return uninstall_virtual_group(target_dir, &virtual_skills, &name, scope);
    }

    // Single skill
    let skill_path = find_installed_skill(target_dir, &name)
        .context(format!("Skill '{}' is not installed", name))?;
    ensure_confined_removal(target_dir, &skill_path, false)?;

    if skill_path.is_symlink() {
        fs::remove_file(&skill_path)?;
    } else {
        fs::remove_dir_all(&skill_path)?;
    }

    // Clean up empty group dir
    if let Some(parent) = skill_path.parent() {
        if parent != target_dir {
            let _ = fs::remove_dir(parent);
        }
    }

    ui::success(&format!(
        "Uninstalled skill '{}' ({}, {})",
        name, scope, agent
    ));
    Ok(())
}

/// Accept only the existing CLI selector forms: `skill`, `group`, `group/skill`,
/// and the explicit group spelling `group/`. Validation happens before any path
/// is joined or filesystem mutation is attempted.
fn validate_uninstall_selector(selector: &str) -> Result<String> {
    if selector.is_empty() || Path::new(selector).is_absolute() {
        bail!(
            "Invalid skill selector '{}': expected a relative skill or group name",
            selector
        );
    }
    if selector.contains('\\') || selector.contains('\0') {
        bail!(
            "Invalid skill selector '{}': path separators are not allowed",
            selector
        );
    }

    let explicit_group = selector.ends_with('/');
    let normalized = selector.strip_suffix('/').unwrap_or(selector);
    let components: Vec<&str> = normalized.split('/').collect();
    if components.is_empty()
        || components.len() > 2
        || components.iter().any(|component| component.is_empty())
        || (explicit_group && components.len() != 1)
    {
        bail!(
            "Invalid skill selector '{}': expected skill, group, group/skill, or group/",
            selector
        );
    }

    for component in &components {
        util::validate_name(component)
            .with_context(|| format!("Invalid skill selector '{}'", selector))?;
    }
    Ok(normalized.to_string())
}

/// Verify the removal target is below the selected skill root. Intermediate
/// symlinks are rejected so recursive deletion cannot be redirected elsewhere.
fn ensure_confined_removal(target_dir: &Path, candidate: &Path, recursive: bool) -> Result<()> {
    if target_dir.is_symlink() {
        bail!(
            "Refusing to remove through symlinked skill root: {}",
            target_dir.display()
        );
    }

    let relative = candidate.strip_prefix(target_dir).with_context(|| {
        format!(
            "Refusing to remove path outside skill root: {}",
            candidate.display()
        )
    })?;
    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components.len() > 2
        || components
            .iter()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!(
            "Refusing to remove malformed path outside skill root: {}",
            candidate.display()
        );
    }

    let mut current = target_dir.to_path_buf();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        if current.is_symlink() {
            bail!(
                "Refusing to remove through symlinked skill directory: {}",
                current.display()
            );
        }
    }
    if recursive && candidate.is_symlink() {
        bail!(
            "Refusing to recursively remove symlinked skill directory: {}",
            candidate.display()
        );
    }
    Ok(())
}

/// Uninstall all skills in a real group directory.
fn uninstall_group(
    target_dir: &Path,
    group_dir: &Path,
    group_name: &str,
    scope: &str,
) -> Result<()> {
    let skills: Vec<String> = fs::read_dir(group_dir)?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    if skills.is_empty() {
        bail!("Group '{}' is empty", group_name);
    }

    if console::Term::stderr().is_term() {
        eprintln!(
            "Will uninstall {} skills from group '{}':",
            skills.len(),
            group_name
        );
        for s in &skills {
            eprintln!("  {}/{}", group_name, s);
        }
        eprintln!();
        let confirmed = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Proceed?")
            .default(true)
            .interact()
            .context("Failed to render confirmation")?;
        if !confirmed {
            ui::info("Cancelled.");
            return Ok(());
        }
    }

    for s in &skills {
        let path = group_dir.join(s);
        ensure_confined_removal(target_dir, &path, false)?;
        if path.is_symlink() || path.is_file() {
            fs::remove_file(&path)?;
        } else {
            fs::remove_dir_all(&path)?;
        }
        ui::success(&format!(
            "Uninstalled skill '{}/{}' ({})",
            group_name, s, scope
        ));
    }
    let _ = fs::remove_dir(group_dir);
    Ok(())
}

/// Find flat (non-grouped) skills whose inferred group matches the given name.
/// "other" matches skills that have no group or can't infer one.
fn find_virtual_group_skills(target_dir: &Path, group_name: &str) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry_name.starts_with('.') {
                continue;
            }
            // Skip real group directories
            if path.is_dir() && !path.join("SKILL.md").exists() {
                continue;
            }
            // Infer group from symlink target
            let inferred = if path.is_symlink() {
                fs::read_link(&path)
                    .ok()
                    .and_then(|target| {
                        target
                            .parent()
                            .and_then(|p| p.file_name().map(|g| g.to_string_lossy().to_string()))
                    })
                    .unwrap_or_else(|| "other".to_string())
            } else {
                "other".to_string()
            };
            if inferred == group_name {
                matches.push(path);
            }
        }
    }
    matches
}

/// Uninstall flat skills that belong to a virtual group.
fn uninstall_virtual_group(
    target_dir: &Path,
    skills: &[PathBuf],
    group_name: &str,
    scope: &str,
) -> Result<()> {
    if console::Term::stderr().is_term() {
        eprintln!(
            "Will uninstall {} skills from '{}':",
            skills.len(),
            group_name
        );
        for s in skills {
            eprintln!("  {}", s.file_name().unwrap_or_default().to_string_lossy());
        }
        eprintln!();
        let confirmed = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Proceed?")
            .default(true)
            .interact()
            .context("Failed to render confirmation")?;
        if !confirmed {
            ui::info("Cancelled.");
            return Ok(());
        }
    }

    for path in skills {
        ensure_confined_removal(target_dir, path, false)?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if path.is_symlink() || path.is_file() {
            fs::remove_file(path)?;
        } else {
            fs::remove_dir_all(path)?;
        }
        ui::success(&format!("Uninstalled skill '{}' ({})", name, scope));
    }
    Ok(())
}

/// Find an installed skill by name. Checks both:
///   target_dir/<name>                  (legacy flat)
///   target_dir/<group>/<name>          (new grouped layout)
///   target_dir/<group>/<name> via "group/name" input
fn find_installed_skill(target_dir: &Path, name: &str) -> Option<PathBuf> {
    // Direct match (flat layout or "group/name" input)
    let direct = target_dir.join(name);
    if direct.exists() || direct.is_symlink() {
        return Some(direct);
    }
    // Search group subdirs
    if let Ok(entries) = fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !path.join("SKILL.md").exists() {
                let candidate = path.join(name);
                if candidate.exists() || candidate.is_symlink() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn install_profile(
    profile_name: &str,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
) -> Result<()> {
    let source_dir = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .context(config::source_dir_hint())?;
    let resolved = config::resolve_profile(profile_name, &source_dir)?;

    let target_dir = config::skill_target(global, agent);
    fs::create_dir_all(&target_dir)?;

    let scope = if global { "global" } else { "local" };
    ui::info(&format!(
        "Installing profile '{}': {} skills ({}, {})",
        resolved.name,
        resolved.skills.len(),
        scope,
        agent
    ));

    let local_dir = config::skill_target(false, agent);
    let global_dir = config::skill_target(true, agent);
    let (installed, skipped) = install_profile_entries_with(
        &source_dir,
        &resolved.skills,
        &target_dir,
        global,
        agent,
        force,
        &local_dir,
        &global_dir,
        |source, destination| util::replace_symlink_transactionally(source, destination),
    )?;

    ui::success(&format!(
        "Profile '{}': {} installed, {} skipped",
        resolved.name, installed, skipped
    ));

    // This source is explicitly configured and application-owned, so its local
    // setup behavior remains supported.
    if let Err(e) = run_manifest_setup(&source_dir, ManifestSource::TrustedLocal) {
        ui::warn(&format!("Post-install setup: {}", e));
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn install_profile_entries_with<R>(
    source_dir: &Path,
    skills: &[(String, String)],
    target_dir: &Path,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    local_dir: &Path,
    global_dir: &Path,
    mut replace: R,
) -> Result<(usize, usize)>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    let mut installed = 0;
    let mut skipped = 0;
    for (group, skill_name) in skills {
        let skill_path = source_dir.join(group).join(skill_name);
        if !skill_path.is_dir() || !skill_path.join("SKILL.md").exists() {
            ui::warn(&format!(
                "Skill '{}/{}' not found, skipping",
                group, skill_name
            ));
            skipped += 1;
            continue;
        }

        // Check cross-scope duplicate
        if !force && warn_cross_scope_duplicate(skill_name, group, global, &local_dir, &global_dir)
        {
            skipped += 1;
            continue;
        }

        let link_path = config::skill_destination(&target_dir, group, skill_name, agent);
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if !force && (link_path.exists() || link_path.is_symlink()) {
            skipped += 1;
            continue;
        }

        create_local_skill_link_with(
            &skill_path,
            &link_path,
            force,
            &format!("{group}/{skill_name}"),
            &mut replace,
        )?;
        installed += 1;
    }
    Ok((installed, skipped))
}

fn interactive_install(global: bool, agent: config::SkillAgent, force: bool) -> Result<()> {
    let source_dir = config::find_source_dir();
    let local_installed = installed_skill_names(&config::skill_target(false, agent));
    let global_installed = installed_skill_names(&config::skill_target(true, agent));

    let selection = if let Some(ref sd) = source_dir {
        ui::interactive::run_interactive_selector(sd, &local_installed, &global_installed)?
    } else {
        let cwd_source = config::find_cwd_source_dir();
        ui::interactive::run_no_source_selector(cwd_source)?
    };

    match selection {
        ui::interactive::InteractiveSelection::Profile(prof_name) => {
            let sd = source_dir.context(config::source_dir_hint())?;
            let resolved = config::resolve_profile(&prof_name, &sd)?;
            if !ui::interactive::confirm_install(&resolved.skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_profile(&prof_name, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Skills(skills) => {
            let sd = source_dir.context(config::source_dir_hint())?;
            if !ui::interactive::confirm_install(&skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_selected_skills(&sd, &skills, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Remote(spec) => {
            install_remote(&spec, global, agent, force, None, None)
        }
        ui::interactive::InteractiveSelection::CloneAndInstall => {
            clone_and_install(global, agent, force)
        }
        ui::interactive::InteractiveSelection::LocalRepo(path) => {
            local_repo_install(&path, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Cancelled => {
            ui::info("Installation cancelled.");
            Ok(())
        }
    }
}

fn clone_and_install(global: bool, agent: config::SkillAgent, force: bool) -> Result<()> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let target = home.join(".agent-skills");

    if target.exists() {
        ui::info(&format!("Skills repo already exists: {}", target.display()));
    } else {
        ui::info("Cloning jiunbae/agent-skills...");
        let status = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "https://github.com/jiunbae/agent-skills.git",
            ])
            .arg(&target)
            .status()
            .context("Failed to run git clone")?;
        if !status.success() {
            bail!("git clone failed");
        }
        ui::success(&format!("Cloned to {}", target.display()));
    }

    // Now run interactive install with the fresh source
    ui::info("Launching interactive installer...");
    eprintln!();
    let local_installed = installed_skill_names(&config::skill_target(false, agent));
    let global_installed = installed_skill_names(&config::skill_target(true, agent));

    let selection =
        ui::interactive::run_interactive_selector(&target, &local_installed, &global_installed)?;

    match selection {
        ui::interactive::InteractiveSelection::Profile(prof_name) => {
            let resolved = config::resolve_profile(&prof_name, &target)?;
            if !ui::interactive::confirm_install(&resolved.skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_profile(&prof_name, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Skills(skills) => {
            if !ui::interactive::confirm_install(&skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_selected_skills(&target, &skills, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Remote(spec) => {
            install_remote(&spec, global, agent, force, None, None)
        }
        _ => {
            ui::info("Installation cancelled.");
            Ok(())
        }
    }
}

fn local_repo_install(
    source_dir: &Path,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
) -> Result<()> {
    ui::info(&format!(
        "Using local skills source: {}",
        source_dir.display()
    ));
    eprintln!();
    let local_installed = installed_skill_names(&config::skill_target(false, agent));
    let global_installed = installed_skill_names(&config::skill_target(true, agent));

    let selection =
        ui::interactive::run_interactive_selector(source_dir, &local_installed, &global_installed)?;

    match selection {
        ui::interactive::InteractiveSelection::Profile(prof_name) => {
            let resolved = config::resolve_profile(&prof_name, source_dir)?;
            if !ui::interactive::confirm_install(&resolved.skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_profile(&prof_name, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Skills(skills) => {
            if !ui::interactive::confirm_install(&skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_selected_skills(source_dir, &skills, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Remote(spec) => {
            install_remote(&spec, global, agent, force, None, None)
        }
        _ => {
            ui::info("Installation cancelled.");
            Ok(())
        }
    }
}

fn install_selected_skills(
    source_dir: &Path,
    skills: &[(String, String)],
    global: bool,
    agent: config::SkillAgent,
    force: bool,
) -> Result<()> {
    let target_dir = config::skill_target(global, agent);
    fs::create_dir_all(&target_dir)?;

    let local_dir = config::skill_target(false, agent);
    let global_dir = config::skill_target(true, agent);
    let (installed, skipped) = install_selected_entries_with(
        source_dir,
        skills,
        &target_dir,
        global,
        agent,
        force,
        &local_dir,
        &global_dir,
        |source, destination| util::replace_symlink_transactionally(source, destination),
    )?;

    ui::success(&format!(
        "Done: {} installed, {} skipped",
        installed, skipped
    ));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn install_selected_entries_with<R>(
    source_dir: &Path,
    skills: &[(String, String)],
    target_dir: &Path,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    local_dir: &Path,
    global_dir: &Path,
    mut replace: R,
) -> Result<(usize, usize)>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    let scope = if global { "global" } else { "local" };
    let mut installed = 0;
    let mut skipped = 0;
    for (group, skill_name) in skills {
        let skill_path = source_dir.join(group).join(skill_name);
        if !skill_path.is_dir() || !skill_path.join("SKILL.md").exists() {
            ui::warn(&format!(
                "Skill '{}/{}' not found, skipping",
                group, skill_name
            ));
            skipped += 1;
            continue;
        }

        // Check cross-scope duplicate
        if !force && warn_cross_scope_duplicate(skill_name, group, global, &local_dir, &global_dir)
        {
            skipped += 1;
            continue;
        }

        let link_path = config::skill_destination(&target_dir, group, skill_name, agent);
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if !force && (link_path.exists() || link_path.is_symlink()) {
            skipped += 1;
            continue;
        }

        create_local_skill_link_with(
            &skill_path,
            &link_path,
            force,
            &format!("{group}/{skill_name}"),
            &mut replace,
        )?;
        ui::success(&format!(
            "Installed skill '{}/{}' ({}, {})",
            group, skill_name, scope, agent
        ));
        installed += 1;
    }

    Ok((installed, skipped))
}

fn list(
    installed: bool,
    local: bool,
    global: bool,
    profiles: bool,
    agent: config::SkillAgent,
    json: bool,
) -> Result<()> {
    if profiles {
        return list_profiles_display(json);
    }
    // Build installed skill sets for status lookup
    let local_dir = config::skill_target(false, agent);
    let global_dir = config::skill_target(true, agent);
    let local_installed = installed_skill_names(&local_dir);
    let global_installed = installed_skill_names(&global_dir);

    let mut entries: Vec<serde_json::Value> = Vec::new();

    // If only showing installed
    if installed || local || global {
        if installed || local {
            list_skills_in_dir(&local_dir, "local", &mut entries)?;
        }
        if installed || global {
            list_skills_in_dir(&global_dir, "global", &mut entries)?;
        }

        // Deduplicate: local scope takes priority over global
        dedup_skill_entries(&mut entries);

        if json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
            return Ok(());
        }
        if entries.is_empty() {
            ui::info("No installed skills found.");
            return Ok(());
        }
        print_flat(&entries);
        return Ok(());
    }

    // Default: grouped view showing all skills with install status
    if let Some(source_dir) = config::find_source_dir().or_else(config::find_cwd_source_dir) {
        let skill_groups = config::skill_groups(&source_dir);
        let mut total = 0usize;
        let mut total_installed = 0usize;

        if json {
            // JSON mode: collect all entries
            for group in &skill_groups {
                let skills = config::skills_in_group(&source_dir, group);
                for skill_name in &skills {
                    let skill_path = source_dir.join(group).join(skill_name);
                    let desc = read_skill_description(&skill_path);
                    let status = if local_installed.contains(&skill_name.to_string()) {
                        "local"
                    } else if global_installed.contains(&skill_name.to_string()) {
                        "global"
                    } else {
                        "available"
                    };
                    entries.push(serde_json::json!({
                        "name": skill_name,
                        "group": group,
                        "status": status,
                        "description": desc,
                    }));
                }
            }
            println!("{}", serde_json::to_string_pretty(&entries)?);
            return Ok(());
        }

        ui::section("Available Skills");

        for group in &skill_groups {
            let skills = config::skills_in_group(&source_dir, group);
            let group_installed: usize = skills
                .iter()
                .filter(|s| local_installed.contains(*s) || global_installed.contains(*s))
                .count();

            total += skills.len();
            total_installed += group_installed;

            ui::subsection(&format!(
                "{}/ ({}/{})",
                group,
                group_installed,
                skills.len()
            ));

            let mut table = ui::table::new_table();
            for skill_name in &skills {
                let status = if local_installed.contains(skill_name) {
                    "L".green().bold().to_string()
                } else if global_installed.contains(skill_name) {
                    "G".blue().bold().to_string()
                } else {
                    "○".dimmed().to_string()
                };
                let desc = read_skill_description(&source_dir.join(group).join(skill_name));
                let desc_styled = desc.dimmed().to_string();
                ui::table::add_row(
                    &mut table,
                    &[status.as_str(), skill_name, desc_styled.as_str()],
                );
            }
            if !skills.is_empty() {
                println!("{table}");
            }
        }

        ui::info(&format!(
            "Total: {} skills, {} installed",
            total, total_installed
        ));
    } else {
        // No source dir — infer groups from symlink targets
        list_skills_in_dir(&local_dir, "local", &mut entries)?;
        list_skills_in_dir(&global_dir, "global", &mut entries)?;

        // Deduplicate: local scope takes priority over global
        dedup_skill_entries(&mut entries);

        if json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
            return Ok(());
        }
        if entries.is_empty() {
            ui::info("No skills found.");
            eprintln!("\nTo see all available skills, clone the skills repo:");
            eprintln!("  git clone https://github.com/jiunbae/agent-skills ~/.agent-skills");
            return Ok(());
        }
        print_grouped_installed(&local_dir, &global_dir);
    }

    Ok(())
}

fn init(agent: config::SkillAgent) -> Result<()> {
    let dir = config::skill_target(false, agent);
    if dir.exists() {
        ui::info(&format!(
            "Skill directory already exists: {}",
            dir.display()
        ));
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    ui::success(&format!("Created skill directory: {}", dir.display()));
    Ok(())
}

fn which(name: &str, agent: config::SkillAgent) -> Result<()> {
    // Check local (grouped then flat)
    let local_dir = config::skill_target(false, agent);
    if let Some(found) = find_installed_skill(&local_dir, name) {
        let resolved = fs::canonicalize(&found).unwrap_or(found);
        println!("{}", resolved.display());
        return Ok(());
    }

    // Check global (grouped then flat)
    let global_dir = config::skill_target(true, agent);
    if let Some(found) = find_installed_skill(&global_dir, name) {
        let resolved = fs::canonicalize(&found).unwrap_or(found);
        println!("{}", resolved.display());
        return Ok(());
    }

    // Check source library
    if let Some(source_dir) = config::find_source_dir().or_else(config::find_cwd_source_dir) {
        if let Some(path) = find_skill_in_source(&source_dir, name) {
            println!("{}", path.display());
            return Ok(());
        }
    }

    bail!("Skill '{}' not found", name);
}

fn update(
    name: Option<String>,
    only_global: bool,
    only_local: bool,
    agent: config::SkillAgent,
) -> Result<()> {
    let name = name
        .as_deref()
        .map(validate_uninstall_selector)
        .transpose()?;
    let mut targets: Vec<(&str, PathBuf)> = Vec::new();

    if !only_global {
        targets.push(("local", config::skill_target(false, agent)));
    }
    if !only_local {
        targets.push(("global", config::skill_target(true, agent)));
    }

    let mut total_updated = 0usize;
    let mut total_failed = 0usize;
    let mut found_any = false;

    for (scope, target_dir) in &targets {
        match update_target_dir_exists(target_dir) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                found_any = true;
                total_failed += 1;
                ui::warn(&format!("Failed to inspect {} skill root: {:#}", scope, e));
                continue;
            }
        }

        let discovered = match &name {
            Some(n) => find_update_targets(target_dir, n),
            None => find_all_remote_skills(target_dir),
        };
        let remote_skills = match discovered {
            Ok(skills) => skills,
            Err(e) => {
                found_any = true;
                total_failed += 1;
                let requested = name.as_deref().unwrap_or("all remote skills");
                ui::warn(&format!(
                    "Failed to inspect '{}' ({}): {:#}",
                    requested, scope, e
                ));
                continue;
            }
        };

        if remote_skills.is_empty() {
            continue;
        }
        found_any = true;

        let (updated, failed) =
            update_skill_batch(&remote_skills, scope, |path, display, scope| {
                update_single_skill(target_dir, path, display, scope)
            });
        total_updated += updated;
        total_failed += failed;
    }

    if !found_any {
        if let Some(ref n) = name {
            bail!("No remote skill '{}' found to update", n);
        } else {
            ui::info("No remote-installed skills found to update.");
        }
    } else {
        ensure_update_success(total_updated, total_failed)?;
        ui::success(&format!(
            "Update complete: {} updated, {} failed",
            total_updated, total_failed
        ));
    }

    Ok(())
}

fn update_target_dir_exists(target_dir: &Path) -> Result<bool> {
    match fs::symlink_metadata(target_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("Skill root is a symlink: {}", target_dir.display())
        }
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => bail!(
            "Skill root exists but is not a directory: {}",
            target_dir.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to inspect skill root {}", target_dir.display())),
    }
}

fn validate_update_path(target_dir: &Path, candidate: &Path) -> Result<()> {
    if !update_target_dir_exists(target_dir)? {
        bail!("Skill root does not exist: {}", target_dir.display());
    }
    let relative = candidate.strip_prefix(target_dir).with_context(|| {
        format!(
            "Update target escapes selected skill root: {}",
            candidate.display()
        )
    })?;
    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components.len() > 2
        || components
            .iter()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("Invalid update target path: {}", candidate.display());
    }

    let mut current = target_dir.to_path_buf();
    for component in components {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Update target traverses a symlink: {}", current.display())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Failed to inspect update path {}", current.display())
                })
            }
        }
    }
    Ok(())
}

fn ensure_update_success(total_updated: usize, total_failed: usize) -> Result<()> {
    if total_failed > 0 {
        bail!(
            "Update complete: {} updated, {} failed",
            total_updated,
            total_failed
        );
    }
    Ok(())
}

fn update_skill_batch<F>(
    remote_skills: &[(PathBuf, String)],
    scope: &str,
    mut update_one: F,
) -> (usize, usize)
where
    F: FnMut(&Path, &str, &str) -> Result<()>,
{
    let mut updated = 0;
    let mut failed = 0;
    for (skill_path, display_name) in remote_skills {
        match update_one(skill_path, display_name, scope) {
            Ok(()) => updated += 1,
            Err(e) => {
                ui::warn(&format!("Failed to update '{}': {:#}", display_name, e));
                failed += 1;
            }
        }
    }
    (updated, failed)
}

fn checked_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::symlink_metadata(path) {
                Err(link_error) if link_error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Ok(_) => bail!("Path is an unresolved symlink: {}", path.display()),
                Err(link_error) => Err(link_error)
                    .with_context(|| format!("Failed to inspect path {}", path.display())),
            }
        }
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect path {}", path.display()))
        }
    }
}

fn collect_directory_paths<I>(entries: I, directory: &Path) -> Result<Vec<PathBuf>>
where
    I: IntoIterator<Item = std::io::Result<PathBuf>>,
{
    entries
        .into_iter()
        .map(|entry| {
            entry.with_context(|| format!("Failed to read entry in {}", directory.display()))
        })
        .collect()
}

fn read_directory_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory {}", directory.display()))?;
    collect_directory_paths(
        entries.map(|entry| entry.map(|entry| entry.path())),
        directory,
    )
}

fn has_remote_metadata(skill_path: &Path) -> Result<bool> {
    match checked_metadata(skill_path)? {
        Some(metadata) if metadata.is_dir() => {
            Ok(checked_metadata(&skill_path.join(".remote-source"))?.is_some())
        }
        _ => Ok(false),
    }
}

/// Scan a target directory for all skills that have .remote-source metadata.
fn find_all_remote_skills(target_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut results = Vec::new();
    if !update_target_dir_exists(target_dir)? {
        return Ok(results);
    }

    for path in read_directory_paths(target_dir)? {
        let entry_metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("Failed to inspect directory entry {}", path.display()))?;
        if entry_metadata.file_type().is_symlink() {
            continue;
        }
        validate_update_path(target_dir, &path)?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if name.starts_with('.') {
            continue;
        }

        let is_group =
            entry_metadata.is_dir() && checked_metadata(&path.join("SKILL.md"))?.is_none();
        if is_group {
            for child_path in read_directory_paths(&path)? {
                let child_metadata = fs::symlink_metadata(&child_path).with_context(|| {
                    format!("Failed to inspect directory entry {}", child_path.display())
                })?;
                if child_metadata.file_type().is_symlink() {
                    continue;
                }
                validate_update_path(target_dir, &child_path)?;
                let child_name = child_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if child_name.starts_with('.') {
                    continue;
                }
                if has_remote_metadata(&child_path)? {
                    results.push((child_path, format!("{}/{}", name, child_name)));
                }
            }
        } else if has_remote_metadata(&path)? {
            results.push((path, name));
        }
    }

    Ok(results)
}

/// Find update targets by name. Handles skill name, group name, or group/name format.
fn find_update_targets(target_dir: &Path, name: &str) -> Result<Vec<(PathBuf, String)>> {
    let name = validate_uninstall_selector(name)?;
    if !update_target_dir_exists(target_dir)? {
        return Ok(Vec::new());
    }

    // "group/skill" format
    if name.contains('/') {
        let path = target_dir.join(&name);
        validate_update_path(target_dir, &path)?;
        if has_remote_metadata(&path)? {
            return Ok(vec![(path, name.to_string())]);
        }
        if checked_metadata(&path)?.is_some() {
            bail!(
                "Skill '{}' is not a remote skill (no .remote-source metadata). \
                 Only remote-installed skills can be updated.",
                name
            );
        }
        return Ok(vec![]);
    }

    // Check if name matches a group directory
    let group_dir = target_dir.join(&name);
    validate_update_path(target_dir, &group_dir)?;
    let group_metadata = checked_metadata(&group_dir)?;
    if group_metadata.is_some_and(|metadata| metadata.is_dir())
        && checked_metadata(&group_dir.join("SKILL.md"))?.is_none()
    {
        let mut results = Vec::new();
        for child_path in read_directory_paths(&group_dir)? {
            validate_update_path(target_dir, &child_path)?;
            let child_name = child_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if child_name.starts_with('.') {
                continue;
            }
            if has_remote_metadata(&child_path)? {
                results.push((child_path, format!("{}/{}", name, child_name)));
            }
        }
        if !results.is_empty() {
            return Ok(results);
        }
    }

    // Check as a single skill
    if let Some(skill_path) = find_installed_skill_for_update(target_dir, &name)? {
        if has_remote_metadata(&skill_path)? {
            let display = skill_path
                .strip_prefix(target_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| name.to_string());
            return Ok(vec![(skill_path, display)]);
        }
        bail!(
            "Skill '{}' is not a remote skill (no .remote-source metadata). \
             Only remote-installed skills can be updated.",
            name
        );
    }

    Ok(vec![])
}

fn find_installed_skill_for_update(target_dir: &Path, name: &str) -> Result<Option<PathBuf>> {
    let direct = target_dir.join(name);
    validate_update_path(target_dir, &direct)?;
    if checked_metadata(&direct)?.is_some() {
        return Ok(Some(direct));
    }

    for path in read_directory_paths(target_dir)? {
        validate_update_path(target_dir, &path)?;
        let entry_name = path.file_name().unwrap_or_default().to_string_lossy();
        if entry_name.starts_with('.') {
            continue;
        }
        let metadata = checked_metadata(&path)?
            .with_context(|| format!("Directory entry disappeared: {}", path.display()))?;
        if metadata.is_dir() && checked_metadata(&path.join("SKILL.md"))?.is_none() {
            let candidate = path.join(name);
            validate_update_path(target_dir, &candidate)?;
            if checked_metadata(&candidate)?.is_some() {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

/// Update a single remote skill by re-fetching from its original source.
fn update_single_skill(
    target_dir: &Path,
    skill_path: &Path,
    display_name: &str,
    scope: &str,
) -> Result<()> {
    validate_update_path(target_dir, skill_path)?;
    let spec = remote::parse_metadata(skill_path)?;
    validate_remote_source_path(&spec)?;

    ui::info(&format!(
        "Updating '{}' ({}) from {}...",
        display_name, scope, spec
    ));

    let (tmp_dir, source_path) = remote::fetch_dir(&spec)?;

    if !source_path.join("SKILL.md").exists() {
        bail!("Remote source no longer contains SKILL.md");
    }
    validate_fetched_skill_path(tmp_dir.path(), &source_path)?;
    validate_update_path(target_dir, skill_path)?;

    util::replace_dir_transactionally(&source_path, skill_path, |staged| {
        if !staged.join("SKILL.md").is_file() {
            bail!("Staged remote skill does not contain SKILL.md");
        }
        remote::write_metadata(staged, &spec)
    })?;

    ui::success(&format!("Updated '{}' ({})", display_name, scope));
    Ok(())
}

fn list_profiles_display(json: bool) -> Result<()> {
    let source_dir = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .context(config::source_dir_hint())?;
    let profiles = config::list_profiles(&source_dir)?;

    if json {
        let entries: Vec<serde_json::Value> = profiles
            .iter()
            .map(|(name, desc, count)| {
                serde_json::json!({
                    "name": name,
                    "description": desc,
                    "skill_count": count,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    let mut table = ui::table::new_table();
    table.set_header(&["Profile", "Description", "Skills"]);
    for (name, desc, count) in &profiles {
        ui::table::add_row(&mut table, &[name, desc, &count.to_string()]);
    }
    println!("{}", "Installation Profiles".cyan().bold());
    println!("{table}");
    println!("Usage: agt skill install --profile <name> [-g]");
    Ok(())
}

// --- Helpers ---

fn find_skill_in_source(source_dir: &Path, name: &str) -> Option<PathBuf> {
    for group in config::skill_groups(source_dir) {
        let path = source_dir.join(&group).join(name);
        if path.is_dir() && path.join("SKILL.md").exists() {
            return Some(path);
        }
    }
    None
}

/// Check if a skill (group/name) exists in a target directory
fn skill_exists_in_dir(dir: &Path, group: &str, name: &str) -> bool {
    // Check grouped layout: dir/group/name
    let grouped = dir.join(group).join(name);
    if grouped.exists() || grouped.is_symlink() {
        return true;
    }
    // Check flat layout: dir/name
    let flat = dir.join(name);
    if flat.exists() || flat.is_symlink() {
        return true;
    }
    false
}

/// Check cross-scope duplicate and print warning. Returns true if duplicate found.
fn warn_cross_scope_duplicate(
    skill_name: &str,
    group: &str,
    installing_global: bool,
    local_dir: &Path,
    global_dir: &Path,
) -> bool {
    // Skip check if both scopes resolve to the same directory
    if let (Ok(l), Ok(g)) = (
        std::fs::canonicalize(local_dir),
        std::fs::canonicalize(global_dir),
    ) {
        if l == g {
            return false;
        }
    }
    let (other_dir, other_scope) = if installing_global {
        (local_dir, "local")
    } else {
        (global_dir, "global")
    };
    if skill_exists_in_dir(other_dir, group, skill_name) {
        eprintln!(
            "{}",
            format!(
                "⚠ Skipped '{}/{}': already installed as {} (use --force to overwrite)",
                group, skill_name, other_scope
            )
            .yellow()
        );
        true
    } else {
        false
    }
}

fn installed_skill_names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            // Check if this is a group directory
            if path.is_dir() && !path.join("SKILL.md").exists() {
                if let Ok(children) = fs::read_dir(&path) {
                    for child in children.flatten() {
                        let child_name = child.file_name().to_string_lossy().to_string();
                        if !child_name.starts_with('.') {
                            names.push(child_name);
                        }
                    }
                }
            } else {
                names.push(name);
            }
        }
    }
    names
}

fn list_skills_in_dir(dir: &Path, scope: &str, entries: &mut Vec<serde_json::Value>) -> Result<()> {
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            // Check if this is a group directory (no SKILL.md)
            if path.is_dir() && !path.join("SKILL.md").exists() {
                if let Ok(children) = fs::read_dir(&path) {
                    for child in children.flatten() {
                        let child_name = child.file_name().to_string_lossy().to_string();
                        if child_name.starts_with('.') {
                            continue;
                        }
                        let child_path = child.path();
                        let desc = read_skill_description(&child_path);
                        let is_remote = child_path.join(".remote-source").exists();
                        let is_symlink = child_path.is_symlink();
                        entries.push(serde_json::json!({
                            "name": format!("{}/{}", name, child_name),
                            "group": name,
                            "scope": scope,
                            "description": desc,
                            "remote": is_remote,
                            "symlink": is_symlink,
                        }));
                    }
                }
                continue;
            }

            let desc = read_skill_description(&path);
            let is_remote = path.join(".remote-source").exists();
            let is_symlink = path.is_symlink();

            entries.push(serde_json::json!({
                "name": name,
                "scope": scope,
                "description": desc,
                "remote": is_remote,
                "symlink": is_symlink,
            }));
        }
    }
    Ok(())
}

/// Deduplicate skill entries by name, keeping the first occurrence (local over global).
fn dedup_skill_entries(entries: &mut Vec<serde_json::Value>) {
    let mut seen = HashSet::new();
    entries.retain(|entry| {
        let name = entry["name"].as_str().unwrap_or("").to_string();
        seen.insert(name)
    });
}

fn read_skill_description(path: &Path) -> String {
    let skill_md = path.join("SKILL.md");
    if let Ok(content) = fs::read_to_string(skill_md) {
        if let Ok((fm, _)) = frontmatter::parse(&content) {
            if let Some(desc) = fm.description {
                return truncate_description(&desc);
            }
        }
    }
    String::new()
}

fn truncate_description(desc: &str) -> String {
    let trimmed = desc.trim();
    if trimmed.chars().count() > 80 {
        let truncated: String = trimmed.chars().take(77).collect();
        format!("{}...", truncated)
    } else {
        trimmed.to_string()
    }
}

fn print_grouped_installed(local_dir: &Path, global_dir: &Path) {
    use std::collections::BTreeMap;

    // Deduplicate: key = "group/skill", value = (group, scope, desc)
    // Local takes priority over global
    let mut seen: BTreeMap<String, (String, String, String)> = BTreeMap::new();

    for (dir, scope) in [(local_dir, "local"), (global_dir, "global")] {
        if let Ok(read) = fs::read_dir(dir) {
            for entry in read.flatten() {
                let entry_name = entry.file_name().to_string_lossy().to_string();
                if entry_name.starts_with('.') {
                    continue;
                }
                let path = entry.path();

                // Check if this is a group directory (contains skill subdirs)
                if path.is_dir() && !path.join("SKILL.md").exists() {
                    // This is a group directory — scan its children
                    if let Ok(children) = fs::read_dir(&path) {
                        for child in children.flatten() {
                            let skill_name = child.file_name().to_string_lossy().to_string();
                            if skill_name.starts_with('.') {
                                continue;
                            }
                            let key = format!("{}/{}", entry_name, skill_name);
                            if seen.contains_key(&key) {
                                continue;
                            }
                            let child_path = child.path();
                            let desc = read_skill_description(&child_path);
                            seen.insert(key, (entry_name.clone(), scope.to_string(), desc));
                        }
                    }
                } else {
                    // Legacy flat layout or symlink — infer group from symlink target
                    let key = format!("_/{}", entry_name);
                    if seen.contains_key(&key) {
                        continue;
                    }
                    let desc = read_skill_description(&path);
                    let group = if path.is_symlink() {
                        fs::read_link(&path)
                            .ok()
                            .and_then(|target| {
                                target.parent().and_then(|p| {
                                    p.file_name().map(|g| g.to_string_lossy().to_string())
                                })
                            })
                            .unwrap_or_else(|| "other".to_string())
                    } else {
                        "other".to_string()
                    };
                    seen.insert(key, (group, scope.to_string(), desc));
                }
            }
        }
    }

    // Group by group name, extract skill name from key
    let mut groups: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (key, (group, scope, _desc)) in &seen {
        let skill_name = key.split('/').last().unwrap_or(key).to_string();
        groups
            .entry(group.clone())
            .or_default()
            .push((skill_name, scope.clone()));
    }

    ui::section("Installed Skills");

    let mut total = 0usize;
    for (group, skills) in &groups {
        total += skills.len();
        ui::subsection(&format!("{}/ ({})", group, skills.len()));
        let mut table = ui::table::new_table();
        for (name, scope) in skills {
            let tag = if scope == "local" {
                "L".green().bold().to_string()
            } else {
                "G".blue().bold().to_string()
            };
            ui::table::add_row(&mut table, &[tag.as_str(), name.as_str()]);
        }
        println!("{table}");
    }

    ui::info(&format!("{} installed", total));
    eprintln!("\nTo see all available skills:");
    eprintln!("  git clone https://github.com/jiunbae/agent-skills ~/.agent-skills");
    eprintln!("  agt skill install            # interactive installer");
}

fn print_flat(entries: &[serde_json::Value]) {
    let mut table = ui::table::new_table();
    table.set_header(&["Skill", "Scope", "Description"]);
    for entry in entries {
        let name = entry["name"].as_str().unwrap_or("");
        let scope = entry["scope"].as_str().unwrap_or("");
        let desc = entry["description"].as_str().unwrap_or("");
        ui::table::add_row(&mut table, &[name, scope, desc]);
    }
    println!("{table}");
}

#[cfg(test)]
mod tests {
    use super::{
        collect_directory_paths, ensure_update_success, find_all_remote_skills,
        find_update_targets, install_profile_entries_with, install_remote_skill_from_source_with,
        install_selected_entries_with, install_single_local_skill_link,
        install_single_local_skill_link_with, remote_skill_group, run_manifest_setup,
        run_manifest_setup_with, skills_named, uninstall_from_target, update_skill_batch,
        update_target_dir_exists, validate_skill_install_plan, validate_uninstall_selector,
        validate_update_path, ManifestSource,
    };
    use crate::config::SkillAgent;
    use anyhow::bail;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    #[test]
    fn forced_local_skill_install_replaces_existing_entry_with_link() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("sentinel"), "old bytes").unwrap();

        install_single_local_skill_link(&source, &destination, true, "group/skill").unwrap();

        assert!(destination.is_symlink());
        assert_eq!(fs::read_link(destination).unwrap(), source);
    }

    fn local_skill_failure_fixture(
        root: &std::path::Path,
    ) -> (PathBuf, PathBuf, PathBuf, Vec<(String, String)>) {
        let source = root.join("source");
        let target = root.join("target");
        let destination = target.join("group/skill");
        fs::create_dir_all(source.join("group/skill")).unwrap();
        fs::write(source.join("group/skill/SKILL.md"), "new bytes").unwrap();
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(&destination, "old bytes").unwrap();
        (
            source,
            target,
            destination,
            vec![("group".to_string(), "skill".to_string())],
        )
    }

    #[test]
    fn forced_single_skill_propagates_candidate_failure_and_preserves_old_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::write(&destination, "old bytes").unwrap();

        let error = install_single_local_skill_link_with(
            &source,
            &destination,
            true,
            "group/skill",
            |_, _| bail!("injected candidate failure"),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected candidate failure"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old bytes");
    }

    #[test]
    fn forced_profile_propagates_candidate_failure_and_preserves_old_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let (source, target, destination, skills) = local_skill_failure_fixture(temp.path());

        let error = install_profile_entries_with(
            &source,
            &skills,
            &target,
            false,
            SkillAgent::Claude,
            true,
            &temp.path().join("unused-local"),
            &temp.path().join("unused-global"),
            |_, _| bail!("injected candidate failure"),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected candidate failure"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old bytes");
    }

    #[test]
    fn forced_selected_skills_propagates_activation_failure_and_preserves_old_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let (source, target, destination, skills) = local_skill_failure_fixture(temp.path());

        let error = install_selected_entries_with(
            &source,
            &skills,
            &target,
            false,
            SkillAgent::Claude,
            true,
            &temp.path().join("unused-local"),
            &temp.path().join("unused-global"),
            |_, _| bail!("injected activation failure"),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected activation failure"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old bytes");
    }

    #[test]
    fn remote_path_preserves_immediate_parent_as_group() {
        assert_eq!(remote_skill_group("common/korean-editor"), "common");
    }

    #[test]
    fn root_remote_path_has_no_group() {
        assert_eq!(remote_skill_group("korean-editor"), "");
    }

    #[test]
    fn requested_remote_name_selects_only_that_skill() {
        let skills = vec![
            ("agents".to_string(), "background-reviewer".to_string()),
            ("common".to_string(), "korean-editor".to_string()),
        ];
        assert_eq!(
            skills_named(&skills, "korean-editor"),
            vec![("common".to_string(), "korean-editor".to_string())]
        );
    }

    #[test]
    fn forced_remote_install_stages_before_replacing_existing_bytes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source");
        let destination = tmp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("SKILL.md"), "new").unwrap();
        fs::write(destination.join("SKILL.md"), "old").unwrap();
        fs::write(destination.join("old.txt"), "keep until activation").unwrap();

        install_remote_skill_from_source_with(&source, &destination, true, |staged| {
            fs::write(staged.join(".remote-source"), "new metadata")?;
            Ok(())
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "new metadata"
        );
        assert!(!destination.join("old.txt").exists());
    }

    #[test]
    fn remote_install_failures_and_no_force_preserve_existing_bytes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source");
        let missing = tmp.path().join("missing");
        let destination = tmp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("SKILL.md"), "new").unwrap();
        fs::write(destination.join("SKILL.md"), "old").unwrap();
        fs::write(destination.join(".remote-source"), "old metadata").unwrap();

        assert!(
            install_remote_skill_from_source_with(&source, &destination, false, |_| Ok(()))
                .is_err()
        );
        assert!(
            install_remote_skill_from_source_with(&missing, &destination, true, |_| Ok(()))
                .is_err()
        );
        assert!(
            install_remote_skill_from_source_with(&source, &destination, true, |_| {
                bail!("injected metadata failure")
            })
            .is_err()
        );

        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "old metadata"
        );
    }

    #[test]
    fn uninstall_selector_accepts_supported_forms() {
        for (selector, expected) in [
            ("skill", "skill"),
            ("group", "group"),
            ("group/skill", "group/skill"),
            ("group/", "group"),
        ] {
            assert_eq!(validate_uninstall_selector(selector).unwrap(), expected);
        }
    }

    #[test]
    fn uninstall_selector_rejects_escaping_and_malformed_forms() {
        for selector in [
            "",
            ".",
            "..",
            "../skill",
            "/tmp/skill",
            "group//skill",
            "group/skill/",
            "group/skill/extra",
            "group\\skill",
        ] {
            assert!(
                validate_uninstall_selector(selector).is_err(),
                "selector should be rejected: {selector}"
            );
        }
    }

    #[test]
    fn uninstall_traversal_cannot_remove_outside_sentinel() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("SKILL.md");
        fs::write(&sentinel, "sentinel").unwrap();

        assert!(uninstall_from_target("../outside", &target, "test", SkillAgent::Claude).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn remote_profile_plan_rejects_source_and_destination_symlink_escapes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let download = tmp.path().join("download");
        let repo = download.join("repo");
        let target = tmp.path().join("target");
        let outside_source = tmp.path().join("outside-source/skill");
        let outside_target = tmp.path().join("outside-target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&outside_source).unwrap();
        fs::create_dir_all(&outside_target).unwrap();
        fs::write(outside_source.join("SKILL.md"), "sentinel").unwrap();
        fs::write(outside_target.join("sentinel"), "outside").unwrap();
        symlink(tmp.path().join("outside-source"), repo.join("group")).unwrap();

        let skills = vec![("group".to_string(), "skill".to_string())];
        assert!(validate_skill_install_plan(
            &download,
            &repo,
            &target,
            &skills,
            SkillAgent::Claude,
        )
        .is_err());
        assert!(!target.exists());

        fs::remove_file(repo.join("group")).unwrap();
        fs::create_dir_all(repo.join("group/skill")).unwrap();
        fs::write(repo.join("group/skill/SKILL.md"), "valid").unwrap();
        fs::create_dir_all(&target).unwrap();
        symlink(&outside_target, target.join("group")).unwrap();
        assert!(validate_skill_install_plan(
            &download,
            &repo,
            &target,
            &skills,
            SkillAgent::Claude,
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(outside_target.join("sentinel")).unwrap(),
            "outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_rejects_symlinked_group_without_touching_outside_sentinel() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(outside.join("skill")).unwrap();
        let sentinel = outside.join("skill/SKILL.md");
        fs::write(&sentinel, "sentinel").unwrap();
        symlink(&outside, target.join("group")).unwrap();

        assert!(uninstall_from_target("group/", &target, "test", SkillAgent::Claude).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_rejects_symlinked_root_without_touching_outside_sentinel() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(outside.join("skill")).unwrap();
        let sentinel = outside.join("skill/SKILL.md");
        fs::write(&sentinel, "sentinel").unwrap();
        symlink(&outside, &target).unwrap();

        assert!(uninstall_from_target("skill", &target, "test", SkillAgent::Claude).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "sentinel");
    }

    #[test]
    fn remote_manifest_setup_is_disabled_before_parsing_or_writing() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("agt.toml"), "this is not valid toml").unwrap();

        run_manifest_setup(tmp.path(), ManifestSource::Remote).unwrap();
    }

    #[test]
    fn trusted_local_manifest_setup_still_copies_to_resolved_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        fs::create_dir_all(source.join("static")).unwrap();
        fs::write(source.join("static/config.txt"), "local").unwrap();
        fs::write(
            source.join("agt.toml"),
            "[[setup.copy]]\nfrom = \"static\"\nto = \"~/.agents\"\nstrategy = \"merge\"\n",
        )
        .unwrap();

        run_manifest_setup_with(&source, ManifestSource::TrustedLocal, |_| target.clone()).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("config.txt")).unwrap(),
            "local"
        );
    }

    #[test]
    fn update_batch_attempts_every_target_and_counts_failures() {
        let targets = vec![
            (PathBuf::from("one"), "one".to_string()),
            (PathBuf::from("two"), "two".to_string()),
            (PathBuf::from("three"), "three".to_string()),
        ];
        let mut attempted = Vec::new();

        let counts = update_skill_batch(&targets, "test", |_path, name, _scope| {
            attempted.push(name.to_string());
            if name == "two" {
                bail!("expected failure");
            }
            Ok(())
        });

        assert_eq!(attempted, ["one", "two", "three"]);
        assert_eq!(counts, (2, 1));
    }

    #[test]
    fn update_failure_summary_returns_an_error() {
        let error = ensure_update_success(2, 1).unwrap_err();
        assert_eq!(error.to_string(), "Update complete: 2 updated, 1 failed");
        ensure_update_success(3, 0).unwrap();
    }

    #[test]
    fn update_discovery_distinguishes_missing_root_from_present_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("missing");
        assert!(!update_target_dir_exists(&missing).unwrap());

        let file_root = tmp.path().join("skills");
        fs::write(&file_root, "not a directory").unwrap();
        assert!(update_target_dir_exists(&file_root).is_err());
        assert!(find_all_remote_skills(&file_root).is_err());
        assert!(find_update_targets(&file_root, "group/skill").is_err());
    }

    #[test]
    fn update_discovery_propagates_directory_entry_errors() {
        let directory = PathBuf::from("test-skills");
        let entries = vec![
            Ok(directory.join("one")),
            Err(std::io::Error::other("injected entry failure")),
            Ok(directory.join("three")),
        ];

        let error = collect_directory_paths(entries, &directory).unwrap_err();
        assert!(error.to_string().contains("Failed to read entry"));
    }

    #[test]
    fn update_discovery_finds_flat_grouped_and_named_remote_skills() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        let flat = target.join("flat");
        let grouped = target.join("group/nested");
        for skill in [&flat, &grouped] {
            fs::create_dir_all(skill).unwrap();
            fs::write(skill.join("SKILL.md"), "skill").unwrap();
            fs::write(skill.join(".remote-source"), "source").unwrap();
        }

        let mut discovered: Vec<String> = find_all_remote_skills(&target)
            .unwrap()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        discovered.sort();
        assert_eq!(discovered, ["flat", "group/nested"]);

        let named = find_update_targets(&target, "group/nested").unwrap();
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].1, "group/nested");
    }

    #[test]
    fn update_selectors_reject_absolute_traversal_and_malformed_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        fs::create_dir_all(&target).unwrap();

        for selector in [
            "../outside",
            "/tmp/skill",
            "group//skill",
            "group/skill/extra",
        ] {
            assert!(
                find_update_targets(&target, selector).is_err(),
                "{selector}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn update_rejects_symlinked_root_intermediate_and_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let outside = tmp.path().join("outside");
        let root_link = tmp.path().join("root-link");
        fs::create_dir_all(outside.join("skill")).unwrap();
        fs::write(outside.join("skill/sentinel"), "outside").unwrap();
        symlink(&outside, &root_link).unwrap();
        assert!(update_target_dir_exists(&root_link).is_err());

        let target = tmp.path().join("skills");
        fs::create_dir_all(&target).unwrap();
        symlink(&outside, target.join("group")).unwrap();
        assert!(find_update_targets(&target, "group/skill").is_err());
        fs::remove_file(target.join("group")).unwrap();
        symlink(outside.join("skill"), target.join("skill")).unwrap();
        assert!(validate_update_path(&target, &target.join("skill")).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("skill/sentinel")).unwrap(),
            "outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn update_all_skips_local_symlinks_and_attempts_remote_directories() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        let local_source = tmp.path().join("local-source");
        let remote = target.join("remote");
        fs::create_dir_all(&local_source).unwrap();
        fs::write(local_source.join("SKILL.md"), "local").unwrap();
        fs::create_dir_all(&remote).unwrap();
        fs::write(remote.join("SKILL.md"), "remote").unwrap();
        fs::write(remote.join(".remote-source"), "source").unwrap();
        symlink(&local_source, target.join("local-link")).unwrap();

        let discovered = find_all_remote_skills(&target).unwrap();
        assert_eq!(discovered, vec![(remote, "remote".to_string())]);

        let mut attempted = Vec::new();
        let counts = update_skill_batch(&discovered, "test", |_path, name, _scope| {
            attempted.push(name.to_string());
            Ok(())
        });
        assert_eq!(attempted, ["remote"]);
        assert_eq!(counts, (1, 0));
        assert!(find_update_targets(&target, "local-link").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn update_all_skips_unresolved_symlinks_but_explicit_selection_rejects_them() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        fs::create_dir_all(&target).unwrap();
        symlink("loop", target.join("loop")).unwrap();

        assert!(find_all_remote_skills(&target).unwrap().is_empty());
        assert!(find_update_targets(&target, "loop/skill").is_err());
    }
}

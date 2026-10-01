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
        /// Install a named profile (core, dev, all, ...) or a comma list (core,dev)
        #[arg(short, long, value_name = "NAME")]
        profile: Option<String>,
        /// Install all available skills
        #[arg(short, long)]
        all: bool,
        /// Remote spec: owner/repo/path[@ref]
        #[arg(long, value_name = "SPEC")]
        from: Option<String>,
        /// Skip the source repo's [[setup.copy]] rules (e.g. static files into ~/.agents)
        #[arg(long)]
        no_static: bool,
    },
    /// Show what agt installed in a skills directory, from which layer, and
    /// what it did not install (unmanaged) or lost (missing)
    Status {
        /// Inspect the global skill directory instead of the project one
        #[arg(short, long)]
        global: bool,
        /// Agent whose skill directory should be inspected
        #[arg(long, value_enum, default_value_t)]
        agent: config::SkillAgent,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Move skills that older agt installed under <group>/<skill> to the flat
    /// <skill> layout that Claude Code actually loads
    Migrate {
        /// Migrate the global skill directory instead of the project one
        #[arg(short, long)]
        global: bool,
        /// Show what would move without changing anything
        #[arg(long)]
        dry_run: bool,
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
            no_static,
        } => {
            // `--all` is the built-in "all" profile
            let profile_name = if all {
                if profile.is_some() {
                    bail!("--all and --profile cannot be used together");
                }
                Some("all".to_string())
            } else {
                profile
            };
            install(name, global, agent, force, profile_name, from, !no_static)
        }
        SkillAction::Migrate { global, dry_run } => migrate(global, dry_run),
        SkillAction::Status {
            global,
            agent,
            json,
        } => status(global, agent, json),
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
    profile_name: Option<String>,
    from: Option<String>,
    run_setup: bool,
) -> Result<()> {
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
            run_setup,
        );
    }

    if let Some(prof_name) = profile_name {
        if name.is_some() {
            bail!("Cannot specify both a skill name and --profile/--all");
        }
        return install_profile(&prof_name, global, agent, force, run_setup);
    }

    let name = match name {
        Some(n) => n,
        None => {
            if !console::Term::stderr().is_term() {
                bail!("Skill name required (or use --profile, --all, --from)");
            }
            return interactive_install(global, agent, force, run_setup);
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

    fs::create_dir_all(&target_dir)?;
    migrate_legacy_destination(&target_dir, &group, &name, agent)?;
    let link_path = config::skill_destination(&target_dir, &name);

    util::ensure_target_clear(&link_path, force, &name)?;

    symlink(&skill_path, &link_path).context(format!(
        "Failed to create symlink: {} -> {}",
        link_path.display(),
        skill_path.display()
    ))?;
    record_installed(
        &target_dir,
        vec![(
            name.clone(),
            skill_record(
                "manual",
                source_dir.display().to_string(),
                format!("{group}/{name}"),
                config::InstallMode::Symlink,
            ),
        )],
    )?;

    let scope = if global { "global" } else { "local" };
    ui::success(&format!(
        "Installed skill '{}/{}' ({}, {})",
        group, name, scope, agent
    ));
    Ok(())
}

fn install_remote(
    spec_str: &str,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    profile: Option<&str>,
    requested_name: Option<&str>,
    run_setup: bool,
) -> Result<()> {
    let spec = remote::parse_spec(spec_str)?;

    // Repo-level: owner/repo with no path — browse all skills
    if spec.path.is_empty() {
        return install_remote_repo(
            &spec,
            global,
            agent,
            force,
            profile,
            requested_name,
            run_setup,
        );
    }
    if profile.is_some() {
        bail!("--profile/--all requires a repository-level --from spec");
    }
    if requested_name.is_some() {
        bail!("A skill name cannot be combined with a path-level --from spec");
    }

    ui::info(&format!("Downloading {}...", spec));

    let (_tmp_dir, source_path) = remote::fetch_dir(&spec)?;

    // Verify it's a skill (has SKILL.md)
    if !source_path.join("SKILL.md").exists() {
        bail!("Remote path does not contain SKILL.md: {}", spec);
    }

    let skill_name = source_path
        .file_name()
        .context("Invalid remote path")?
        .to_string_lossy()
        .to_string();
    util::validate_name(&skill_name)?;

    let target_dir = config::skill_target(global, agent);

    let group = remote_skill_group(&spec.path);
    fs::create_dir_all(&target_dir)?;
    migrate_legacy_destination(&target_dir, &group, &skill_name, agent)?;
    let dest = config::skill_destination(&target_dir, &skill_name);

    util::ensure_target_clear(&dest, force, &skill_name)?;

    util::copy_dir_recursive(&source_path, &dest)?;
    remote::write_metadata(&dest, &spec)?;
    record_installed(
        &target_dir,
        vec![(
            skill_name.clone(),
            skill_record(
                "manual",
                format!("{}/{}", spec.owner, spec.repo),
                spec.path.clone(),
                config::InstallMode::Copy,
            ),
        )],
    )?;

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

fn install_remote_repo(
    spec: &remote::RemoteSpec,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    profile: Option<&str>,
    requested_name: Option<&str>,
    run_setup: bool,
) -> Result<()> {
    ui::info(&format!(
        "Downloading {}/{}@{}...",
        spec.owner, spec.repo, spec.git_ref
    ));
    let (_tmp_dir, repo_root) = remote::fetch_dir(spec)?;

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
            .map(|rd| rd.flatten().any(|e| !e.file_name().to_string_lossy().starts_with('.')))
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
    fs::create_dir_all(&target_dir)?;

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
            &repo_root, &local_installed, &global_installed,
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

    let mut installed = 0;
    let mut skipped = 0;
    let mut recorded = Vec::new();
    let local_dir = config::skill_target(false, agent);
    let global_dir = config::skill_target(true, agent);

    for (group, skill_name) in &skills_to_install {
        let source_path = repo_root.join(group).join(skill_name);
        if !source_path.is_dir() || !source_path.join("SKILL.md").exists() {
            skipped += 1;
            continue;
        }

        // Check cross-scope duplicate
        if !force
            && warn_cross_scope_duplicate(skill_name, group, global, &local_dir, &global_dir)
        {
            skipped += 1;
            continue;
        }

        migrate_legacy_destination(&target_dir, group, skill_name, agent)?;
        let dest = config::skill_destination(&target_dir, skill_name);

        if dest.exists() || dest.is_symlink() {
            if force {
                if dest.is_symlink() || dest.is_file() {
                    fs::remove_file(&dest)?;
                } else {
                    fs::remove_dir_all(&dest)?;
                }
            } else {
                skipped += 1;
                continue;
            }
        }

        util::copy_dir_recursive(&source_path, &dest)?;
        let skill_spec = remote::RemoteSpec {
            owner: spec.owner.clone(),
            repo: spec.repo.clone(),
            path: format!("{}/{}", group, skill_name),
            git_ref: spec.git_ref.clone(),
        };
        remote::write_metadata(&dest, &skill_spec)?;
        recorded.push((
            skill_name.clone(),
            skill_record(
                profile.unwrap_or("manual"),
                format!("{}/{}", spec.owner, spec.repo),
                skill_spec.path.clone(),
                config::InstallMode::Copy,
            ),
        ));
        ui::success(&format!(
            "Installed skill '{}/{}' ({}, {})",
            group, skill_name, scope, agent
        ));
        installed += 1;
    }
    record_installed(&target_dir, recorded)?;

    ui::success(&format!(
        "Done: {} installed, {} skipped from {}/{}",
        installed, skipped, spec.owner, spec.repo
    ));

    // Run post-install setup from agt.toml manifest
    if run_setup {
        if let Err(e) = run_manifest_setup(&repo_root) {
            ui::warn(&format!("Post-install setup: {}", e));
        }
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

/// Execute [[setup.copy]] rules from agt.toml in the given directory.
fn run_manifest_setup(repo_root: &Path) -> Result<()> {
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

        let target = config::resolve_home(&rule.to);

        // If target is a symlink, user manages it — skip
        if target.is_symlink() {
            ui::info(&format!(
                "{} is a symlink, skipping",
                rule.to
            ));
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
    let name = name.trim_end_matches('/');
    let target_dir = config::skill_target(global, agent);
    let scope = if global { "global" } else { "local" };

    // Check if name matches a real group directory (e.g. "acme/")
    let group_dir = target_dir.join(name);
    if group_dir.is_dir() && !group_dir.join("SKILL.md").exists() {
        return uninstall_group(&group_dir, name, scope);
    }

    // Check if name matches a virtual group (e.g. "other" — flat skills with inferred group).
    // An installed skill with exactly this name wins over a group of the same name.
    let is_skill = target_dir.join(name).join("SKILL.md").exists();
    let virtual_skills = find_virtual_group_skills(&target_dir, name);
    if !is_skill && !virtual_skills.is_empty() {
        return uninstall_virtual_group(&virtual_skills, name, scope);
    }

    // Single skill
    let skill_path = find_installed_skill(&target_dir, name)
        .context(format!("Skill '{}' is not installed", name))?;

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
    if let Some(file_name) = skill_path.file_name() {
        forget_installed(&target_dir, &[file_name.to_string_lossy().to_string()])?;
    }

    ui::success(&format!(
        "Uninstalled skill '{}' ({}, {})",
        name, scope, agent
    ));
    Ok(())
}

/// Uninstall all skills in a real group directory.
fn uninstall_group(group_dir: &Path, group_name: &str, scope: &str) -> Result<()> {
    let skills: Vec<String> = fs::read_dir(group_dir)?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    if skills.is_empty() {
        bail!("Group '{}' is empty", group_name);
    }

    if console::Term::stderr().is_term() {
        eprintln!("Will uninstall {} skills from group '{}':", skills.len(), group_name);
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
        if path.is_symlink() || path.is_file() {
            fs::remove_file(&path)?;
        } else {
            fs::remove_dir_all(&path)?;
        }
        ui::success(&format!("Uninstalled skill '{}/{}' ({})", group_name, s, scope));
    }
    let _ = fs::remove_dir(group_dir);
    if let Some(target_dir) = group_dir.parent() {
        forget_installed(target_dir, &skills)?;
    }
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
                        target.parent().and_then(|p| {
                            p.file_name().map(|g| g.to_string_lossy().to_string())
                        })
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
fn uninstall_virtual_group(skills: &[PathBuf], group_name: &str, scope: &str) -> Result<()> {
    if console::Term::stderr().is_term() {
        eprintln!("Will uninstall {} skills from '{}':", skills.len(), group_name);
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

    let mut removed = Vec::new();
    for path in skills {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if path.is_symlink() || path.is_file() {
            fs::remove_file(path)?;
        } else {
            fs::remove_dir_all(path)?;
        }
        ui::success(&format!("Uninstalled skill '{}' ({})", name, scope));
        removed.push(name);
    }
    if let Some(target_dir) = skills.first().and_then(|p| p.parent()) {
        forget_installed(target_dir, &removed)?;
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

/// Move a skill that older agt installed at `<target>/<group>/<skill>` to the
/// flat `<target>/<skill>` path Claude Code discovers. Does nothing when the
/// flat path is already taken, so a user-placed skill is never overwritten.
fn migrate_legacy_destination(
    target_dir: &Path,
    group: &str,
    skill_name: &str,
    agent: config::SkillAgent,
) -> Result<()> {
    if agent != config::SkillAgent::Claude || group.is_empty() {
        return Ok(());
    }
    let group_dir = target_dir.join(group);
    if group_dir.is_symlink() || group_dir.join("SKILL.md").exists() {
        return Ok(());
    }
    let legacy = config::legacy_grouped_destination(target_dir, group, skill_name);
    let flat = config::skill_destination(target_dir, skill_name);
    if !(legacy.exists() || legacy.is_symlink()) || flat.exists() || flat.is_symlink() {
        return Ok(());
    }
    fs::rename(&legacy, &flat).context(format!(
        "Failed to move {} -> {}",
        legacy.display(),
        flat.display()
    ))?;
    // Removes the group directory only once it is empty.
    let _ = fs::remove_dir(&group_dir);
    Ok(())
}

/// Claude Code skill directory names: lowercase letters, digits and hyphens.
/// Anything else under a group dir (backups, notes) is left where it is.
fn is_skill_dir_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Default, PartialEq)]
struct MigrationPlan {
    moves: Vec<(PathBuf, PathBuf)>,
    conflicts: Vec<(PathBuf, PathBuf)>,
    ignored: Vec<PathBuf>,
}

/// Find every `<target>/<group>/<skill>` entry that should become `<target>/<skill>`.
fn plan_migration(target_dir: &Path) -> MigrationPlan {
    let mut plan = MigrationPlan::default();
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let Ok(groups) = fs::read_dir(target_dir) else {
        return plan;
    };
    let mut groups: Vec<_> = groups.flatten().map(|e| e.path()).collect();
    groups.sort();
    for group_dir in groups {
        let group_name = group_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if group_name.starts_with('.')
            || group_dir.is_symlink()
            || !group_dir.is_dir()
            || group_dir.join("SKILL.md").exists()
        {
            continue;
        }
        let Ok(children) = fs::read_dir(&group_dir) else {
            continue;
        };
        let mut children: Vec<_> = children.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            let name = child
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') || !child.join("SKILL.md").exists() {
                continue;
            }
            if !is_skill_dir_name(&name) {
                plan.ignored.push(child);
                continue;
            }
            let flat = target_dir.join(&name);
            if flat.exists() || flat.is_symlink() || !claimed.insert(flat.clone()) {
                plan.conflicts.push((child, flat));
            } else {
                plan.moves.push((child, flat));
            }
        }
    }
    plan
}

fn migrate(global: bool, dry_run: bool) -> Result<()> {
    let target_dir = config::skill_target(global, config::SkillAgent::Claude);
    let plan = plan_migration(&target_dir);
    if plan.moves.is_empty() && plan.conflicts.is_empty() && plan.ignored.is_empty() {
        ui::success(&format!(
            "{} already uses the flat layout",
            target_dir.display()
        ));
        return Ok(());
    }

    for (from, to) in &plan.moves {
        if !dry_run {
            fs::rename(from, to).context(format!(
                "Failed to move {} -> {}",
                from.display(),
                to.display()
            ))?;
            if let Some(parent) = from.parent() {
                let _ = fs::remove_dir(parent);
            }
        }
        eprintln!(
            "  {} {} -> {}",
            if dry_run { "would move" } else { "moved" },
            from.strip_prefix(&target_dir).unwrap_or(from).display(),
            to.strip_prefix(&target_dir).unwrap_or(to).display()
        );
    }
    for (from, to) in &plan.conflicts {
        ui::warn(&format!(
            "Left {}: {} is already taken",
            from.strip_prefix(&target_dir).unwrap_or(from).display(),
            to.strip_prefix(&target_dir).unwrap_or(to).display()
        ));
    }
    for path in &plan.ignored {
        ui::warn(&format!(
            "Left {}: not a valid skill directory name",
            path.strip_prefix(&target_dir).unwrap_or(path).display()
        ));
    }

    let verb = if dry_run { "would move" } else { "moved" };
    ui::success(&format!(
        "{} skills {}, {} conflicts, {} ignored ({})",
        plan.moves.len(),
        verb,
        plan.conflicts.len(),
        plan.ignored.len(),
        target_dir.display()
    ));
    Ok(())
}

fn install_profile(
    profile_name: &str,
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    run_setup: bool,
) -> Result<()> {
    let source_dir = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .context(config::source_dir_hint())?;
    let resolved = config::resolve_profile(profile_name, &source_dir)?;

    let scope = if global { "global" } else { "local" };
    ui::info(&format!(
        "Installing profile '{}': {} skills ({}, {})",
        resolved.name,
        resolved.skills.len(),
        scope,
        agent
    ));

    let (installed, skipped) = link_skills(
        &source_dir,
        &resolved.skills,
        global,
        agent,
        force,
        &resolved.name,
        false,
    )?;

    ui::success(&format!(
        "Profile '{}': {} installed, {} skipped",
        resolved.name, installed, skipped
    ));

    // Run post-install setup from agt.toml manifest
    if run_setup {
        if let Err(e) = run_manifest_setup(&source_dir) {
            ui::warn(&format!("Post-install setup: {}", e));
        }
    }

    Ok(())
}

fn interactive_install(
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    run_setup: bool,
) -> Result<()> {
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
            install_profile(&prof_name, global, agent, force, run_setup)
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
            install_remote(&spec, global, agent, force, None, None, run_setup)
        }
        ui::interactive::InteractiveSelection::CloneAndInstall => {
            clone_and_install(global, agent, force, run_setup)
        }
        ui::interactive::InteractiveSelection::LocalRepo(path) => {
            local_repo_install(&path, global, agent, force, run_setup)
        }
        ui::interactive::InteractiveSelection::Cancelled => {
            ui::info("Installation cancelled.");
            Ok(())
        }
    }
}

fn clone_and_install(
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    run_setup: bool,
) -> Result<()> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let target = home.join(".agent-skills");

    if target.exists() {
        ui::info(&format!("Skills repo already exists: {}", target.display()));
    } else {
        ui::info("Cloning jiunbae/agent-skills...");
        let status = std::process::Command::new("git")
            .args(["clone", "--depth", "1", "https://github.com/jiunbae/agent-skills.git"])
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
            install_profile(&prof_name, global, agent, force, run_setup)
        }
        ui::interactive::InteractiveSelection::Skills(skills) => {
            if !ui::interactive::confirm_install(&skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_selected_skills(&target, &skills, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Remote(spec) => {
            install_remote(&spec, global, agent, force, None, None, run_setup)
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
    run_setup: bool,
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
            install_profile(&prof_name, global, agent, force, run_setup)
        }
        ui::interactive::InteractiveSelection::Skills(skills) => {
            if !ui::interactive::confirm_install(&skills, global)? {
                ui::info("Installation cancelled.");
                return Ok(());
            }
            install_selected_skills(source_dir, &skills, global, agent, force)
        }
        ui::interactive::InteractiveSelection::Remote(spec) => {
            install_remote(&spec, global, agent, force, None, None, run_setup)
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
    let (installed, skipped) =
        link_skills(source_dir, skills, global, agent, force, "manual", true)?;

    ui::success(&format!("Done: {} installed, {} skipped", installed, skipped));
    Ok(())
}

/// Symlink `(group, skill)` pairs from a local source into the target skills
/// directory and record them in its state file under `layer`. Existing entries
/// are skipped unless `force`. Returns `(installed, skipped)`.
fn link_skills(
    source_dir: &Path,
    skills: &[(String, String)],
    global: bool,
    agent: config::SkillAgent,
    force: bool,
    layer: &str,
    announce: bool,
) -> Result<(usize, usize)> {
    let target_dir = config::skill_target(global, agent);
    fs::create_dir_all(&target_dir)?;

    let scope = if global { "global" } else { "local" };
    let local_dir = config::skill_target(false, agent);
    let global_dir = config::skill_target(true, agent);
    let mut installed = 0;
    let mut skipped = 0;
    let mut recorded = Vec::new();

    for (group, skill_name) in skills {
        let skill_path = source_dir.join(group).join(skill_name);
        if !skill_path.is_dir() || !skill_path.join("SKILL.md").exists() {
            ui::warn(&format!("Skill '{}/{}' not found, skipping", group, skill_name));
            skipped += 1;
            continue;
        }

        // Check cross-scope duplicate
        if !force
            && warn_cross_scope_duplicate(skill_name, group, global, &local_dir, &global_dir)
        {
            skipped += 1;
            continue;
        }

        migrate_legacy_destination(&target_dir, group, skill_name, agent)?;
        let link_path = config::skill_destination(&target_dir, skill_name);

        if link_path.exists() || link_path.is_symlink() {
            if force {
                if link_path.is_symlink() || link_path.is_file() {
                    fs::remove_file(&link_path)?;
                } else {
                    fs::remove_dir_all(&link_path)?;
                }
            } else {
                skipped += 1;
                continue;
            }
        }

        symlink(&skill_path, &link_path).context(format!(
            "Failed to create symlink for '{}/{}'",
            group, skill_name
        ))?;
        recorded.push((
            skill_name.clone(),
            skill_record(
                layer,
                source_dir.display().to_string(),
                format!("{group}/{skill_name}"),
                config::InstallMode::Symlink,
            ),
        ));
        if announce {
            ui::success(&format!(
                "Installed skill '{}/{}' ({}, {})",
                group, skill_name, scope, agent
            ));
        }
        installed += 1;
    }

    record_installed(&target_dir, recorded)?;
    Ok((installed, skipped))
}

#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum Health {
    /// Recorded in state and present
    Ok,
    /// Recorded in state but gone or a dangling symlink
    Missing,
    /// Present but not installed by agt
    Unmanaged,
}

#[derive(Debug, serde::Serialize)]
struct StatusRow {
    name: String,
    health: Health,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<config::SkillRecord>,
}

/// Compare a skills directory with its state file.
fn status_rows(target_dir: &Path, state: &config::InstallState) -> Vec<StatusRow> {
    let mut rows = Vec::new();
    for (name, record) in &state.skills {
        let path = config::skill_destination(target_dir, name);
        let health = if path.join("SKILL.md").exists() {
            Health::Ok
        } else {
            Health::Missing
        };
        rows.push(StatusRow {
            name: name.clone(),
            health,
            record: Some(record.clone()),
        });
    }
    if let Ok(entries) = fs::read_dir(target_dir) {
        let mut unmanaged: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join("SKILL.md").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| !name.starts_with('.') && !state.skills.contains_key(name))
            .collect();
        unmanaged.sort();
        rows.extend(unmanaged.into_iter().map(|name| StatusRow {
            name,
            health: Health::Unmanaged,
            record: None,
        }));
    }
    rows
}

fn status(global: bool, agent: config::SkillAgent, json: bool) -> Result<()> {
    let target_dir = config::skill_target(global, agent);
    let state = config::InstallState::load(&target_dir)?;
    let rows = status_rows(&target_dir, &state);
    let legacy = if agent == config::SkillAgent::Claude {
        plan_migration(&target_dir).moves.len()
    } else {
        0
    };

    if json {
        let out = serde_json::json!({
            "target": target_dir,
            "state": config::state_path(&target_dir),
            "stack": state.stack,
            "skills": rows,
            "legacy_grouped": legacy,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    eprintln!("{} {}", "Target:".bold(), target_dir.display());
    if let Some(stack) = &state.stack {
        eprintln!("{} {}", "Stack:".bold(), stack);
    }
    if rows.is_empty() {
        ui::info("No skills installed");
    }
    for row in &rows {
        let mark = match row.health {
            Health::Ok => "ok".green(),
            Health::Missing => "missing".red(),
            Health::Unmanaged => "unmanaged".yellow(),
        };
        let detail = match &row.record {
            Some(r) => format!("{}  {} ({})", r.layer, r.origin, r.source),
            None => "not installed by agt".to_string(),
        };
        eprintln!("  {:<10} {:<28} {}", mark, row.name, detail.dimmed());
    }
    if legacy > 0 {
        ui::warn(&format!(
            "{} skills sit under <group>/<skill> where Claude Code does not load them; run `agt skill migrate{}`",
            legacy,
            if global { " --global" } else { "" }
        ));
    }
    Ok(())
}

fn skill_record(
    layer: &str,
    source: String,
    origin: String,
    mode: config::InstallMode,
) -> config::SkillRecord {
    config::SkillRecord {
        layer: layer.to_string(),
        source,
        origin,
        mode,
    }
}

/// Add freshly installed skills to the target's state file.
fn record_installed(target_dir: &Path, entries: Vec<(String, config::SkillRecord)>) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut state = config::InstallState::load(target_dir)?;
    for (name, record) in entries {
        state.record(&name, record);
    }
    state.save(target_dir)
}

/// Drop removed skills from the target's state file.
fn forget_installed(target_dir: &Path, names: &[String]) -> Result<()> {
    let mut state = config::InstallState::load(target_dir)?;
    let mut changed = false;
    for name in names {
        changed |= state.forget(name);
    }
    if changed {
        state.save(target_dir)?;
    }
    Ok(())
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

            ui::subsection(&format!("{}/ ({}/{})", group, group_installed, skills.len()));

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
                ui::table::add_row(&mut table, &[
                    status.as_str(),
                    skill_name,
                    desc_styled.as_str(),
                ]);
            }
            if !skills.is_empty() {
                println!("{table}");
            }
        }

        ui::info(&format!("Total: {} skills, {} installed", total, total_installed));
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
        ui::info(&format!("Skill directory already exists: {}", dir.display()));
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
        if !target_dir.is_dir() {
            continue;
        }

        let remote_skills = match &name {
            Some(n) => find_update_targets(target_dir, n)?,
            None => find_all_remote_skills(target_dir),
        };

        if remote_skills.is_empty() {
            continue;
        }
        found_any = true;

        for (skill_path, display_name) in &remote_skills {
            match update_single_skill(skill_path, display_name, scope) {
                Ok(()) => total_updated += 1,
                Err(e) => {
                    ui::warn(&format!("Failed to update '{}': {:#}", display_name, e));
                    total_failed += 1;
                }
            }
        }
    }

    if !found_any {
        if let Some(ref n) = name {
            bail!("No remote skill '{}' found to update", n);
        } else {
            ui::info("No remote-installed skills found to update.");
        }
    } else {
        ui::success(&format!(
            "Update complete: {} updated, {} failed",
            total_updated, total_failed
        ));
    }

    Ok(())
}

/// Scan a target directory for all skills that have .remote-source metadata.
fn find_all_remote_skills(target_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();

    if let Ok(entries) = fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            // Group directory (no SKILL.md) — scan children
            if path.is_dir() && !path.join("SKILL.md").exists() {
                if let Ok(children) = fs::read_dir(&path) {
                    for child in children.flatten() {
                        let child_path = child.path();
                        let child_name = child.file_name().to_string_lossy().to_string();
                        if child_name.starts_with('.') {
                            continue;
                        }
                        if child_path.join(".remote-source").exists() {
                            results.push((child_path, format!("{}/{}", name, child_name)));
                        }
                    }
                }
            } else if path.join(".remote-source").exists() {
                results.push((path, name));
            }
        }
    }

    results
}

/// Find update targets by name. Handles skill name, group name, or group/name format.
fn find_update_targets(target_dir: &Path, name: &str) -> Result<Vec<(PathBuf, String)>> {
    let name = name.trim_end_matches('/');

    // "group/skill" format
    if name.contains('/') {
        let path = target_dir.join(name);
        if path.join(".remote-source").exists() {
            return Ok(vec![(path, name.to_string())]);
        }
        if path.exists() {
            bail!(
                "Skill '{}' is not a remote skill (no .remote-source metadata). \
                 Only remote-installed skills can be updated.",
                name
            );
        }
        return Ok(vec![]);
    }

    // Check if name matches a group directory
    let group_dir = target_dir.join(name);
    if group_dir.is_dir() && !group_dir.join("SKILL.md").exists() {
        let mut results = Vec::new();
        if let Ok(children) = fs::read_dir(&group_dir) {
            for child in children.flatten() {
                let child_path = child.path();
                let child_name = child.file_name().to_string_lossy().to_string();
                if child_name.starts_with('.') {
                    continue;
                }
                if child_path.join(".remote-source").exists() {
                    results.push((child_path, format!("{}/{}", name, child_name)));
                }
            }
        }
        if !results.is_empty() {
            return Ok(results);
        }
    }

    // Check as a single skill
    if let Some(skill_path) = find_installed_skill(target_dir, name) {
        if skill_path.join(".remote-source").exists() {
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

/// Update a single remote skill by re-fetching from its original source.
fn update_single_skill(skill_path: &Path, display_name: &str, scope: &str) -> Result<()> {
    let spec = remote::parse_metadata(skill_path)?;

    ui::info(&format!(
        "Updating '{}' ({}) from {}...",
        display_name, scope, spec
    ));

    let (_tmp_dir, source_path) = remote::fetch_dir(&spec)?;

    if !source_path.join("SKILL.md").exists() {
        bail!("Remote source no longer contains SKILL.md");
    }

    // Replace: remove old, copy new
    if skill_path.is_dir() {
        fs::remove_dir_all(skill_path)?;
    }
    util::copy_dir_recursive(&source_path, skill_path)?;
    remote::write_metadata(skill_path, &spec)?;

    ui::success(&format!("Updated '{}' ({})", display_name, scope));
    Ok(())
}

fn list_profiles_display(json: bool) -> Result<()> {
    let source_dir = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .context(config::source_dir_hint())?;
    let profiles = config::list_profiles(&source_dir);

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

fn list_skills_in_dir(
    dir: &Path,
    scope: &str,
    entries: &mut Vec<serde_json::Value>,
) -> Result<()> {
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
        is_skill_dir_name, migrate_legacy_destination, plan_migration, remote_skill_group,
        skills_named,
    };
    use crate::config::SkillAgent;
    use std::fs;
    use std::path::Path;

    fn make_skill(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
    }

    #[test]
    fn legacy_grouped_skill_moves_to_flat_path() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(&tmp.path().join("development/git-commit-pr"));

        migrate_legacy_destination(
            tmp.path(),
            "development",
            "git-commit-pr",
            SkillAgent::Claude,
        )
        .unwrap();

        assert!(tmp.path().join("git-commit-pr/SKILL.md").exists());
        assert!(!tmp.path().join("development").exists());
    }

    #[test]
    fn legacy_move_never_overwrites_flat_skill() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(&tmp.path().join("development/git-commit-pr"));
        make_skill(&tmp.path().join("git-commit-pr"));

        migrate_legacy_destination(
            tmp.path(),
            "development",
            "git-commit-pr",
            SkillAgent::Claude,
        )
        .unwrap();

        assert!(tmp.path().join("development/git-commit-pr").exists());
    }

    #[test]
    fn codex_targets_are_not_migrated() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(&tmp.path().join("development/git-commit-pr"));

        migrate_legacy_destination(
            tmp.path(),
            "development",
            "git-commit-pr",
            SkillAgent::Codex,
        )
        .unwrap();

        assert!(tmp.path().join("development/git-commit-pr").exists());
    }

    #[test]
    fn plan_reports_moves_conflicts_and_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(&tmp.path().join("agents/rpf"));
        make_skill(&tmp.path().join("agents/rpf.backup.20260811"));
        make_skill(&tmp.path().join("common/korean-editor"));
        make_skill(&tmp.path().join("korean-editor"));
        make_skill(&tmp.path().join("nlc"));

        let plan = plan_migration(tmp.path());

        assert_eq!(
            plan.moves,
            vec![(tmp.path().join("agents/rpf"), tmp.path().join("rpf"))]
        );
        assert_eq!(
            plan.conflicts,
            vec![(
                tmp.path().join("common/korean-editor"),
                tmp.path().join("korean-editor")
            )]
        );
        assert_eq!(
            plan.ignored,
            vec![tmp.path().join("agents/rpf.backup.20260811")]
        );
    }

    #[test]
    fn plan_flags_same_name_in_two_groups_as_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(&tmp.path().join("a/static-index"));
        make_skill(&tmp.path().join("b/static-index"));

        let plan = plan_migration(tmp.path());

        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.conflicts.len(), 1);
    }

    #[test]
    fn status_separates_ok_missing_and_unmanaged() {
        use crate::config::{InstallMode, InstallState, SkillRecord};
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        make_skill(&skills.join("kept"));
        make_skill(&skills.join("by-hand"));
        let mut state = InstallState::default();
        for name in ["kept", "gone"] {
            state.record(
                name,
                SkillRecord {
                    layer: "core".into(),
                    source: "/src".into(),
                    origin: format!("dev/{name}"),
                    mode: InstallMode::Symlink,
                },
            );
        }

        let rows: Vec<_> = super::status_rows(&skills, &state)
            .into_iter()
            .map(|r| (r.name, r.health))
            .collect();

        assert_eq!(
            rows,
            vec![
                ("gone".to_string(), super::Health::Missing),
                ("kept".to_string(), super::Health::Ok),
                ("by-hand".to_string(), super::Health::Unmanaged),
            ]
        );
    }

    #[test]
    fn skill_dir_names_follow_claude_rules() {
        assert!(is_skill_dir_name("git-commit-pr"));
        assert!(is_skill_dir_name("rpf"));
        assert!(!is_skill_dir_name("rpf.backup.20260811"));
        assert!(!is_skill_dir_name("Upper"));
        assert!(!is_skill_dir_name("-lead"));
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
}

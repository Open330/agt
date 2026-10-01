use crate::{config, frontmatter, llm, remote, ui, util};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// Check if the static-index skill is installed (local or global)
fn is_static_index_installed() -> bool {
    let local = config::local_skill_target();
    let global = config::global_skill_target();
    let codex_local = config::local_codex_skill_target();
    let codex_global = config::global_codex_skill_target();

    // Check grouped layout: context/static-index
    let local_grouped = local.join("context").join("static-index");
    let global_grouped = global.join("context").join("static-index");
    // Check flat layout: static-index
    let local_flat = local.join("static-index");
    let global_flat = global.join("static-index");
    let codex_local_flat = codex_local.join("static-index");
    let codex_global_flat = codex_global.join("static-index");

    local_grouped.exists()
        || local_grouped.is_symlink()
        || global_grouped.exists()
        || global_grouped.is_symlink()
        || local_flat.exists()
        || local_flat.is_symlink()
        || global_flat.exists()
        || global_flat.is_symlink()
        || codex_local_flat.exists()
        || codex_local_flat.is_symlink()
        || codex_global_flat.exists()
        || codex_global_flat.is_symlink()
}

/// Suggest installing static-index skill if not present.
fn suggest_static_index() {
    if is_static_index_installed() {
        return;
    }

    ui::hint(
        "The 'static-index' skill is not installed. \
         Claude uses it to discover persona locations.",
    );
    ui::hint("Install it with: agt skill install static-index --global");
}

/// Post-install action: suggest static-index without executing source-tree scripts.
fn post_persona_install() {
    suggest_static_index();
}

#[derive(Subcommand)]
pub enum PersonaAction {
    /// Install a persona to .agents/personas/ (local) or ~/.agents/personas/ (global)
    Install {
        /// Persona name (from library)
        name: Option<String>,
        /// Install globally to ~/.agents/personas/
        #[arg(short, long)]
        global: bool,
        /// Force overwrite existing
        #[arg(short, long)]
        force: bool,
        /// Install all library personas
        #[arg(short, long)]
        all: bool,
        /// Remote spec: owner/repo/path[@ref]
        #[arg(long, value_name = "SPEC")]
        from: Option<String>,
    },
    /// Uninstall a persona
    Uninstall {
        /// Persona name
        name: Option<String>,
        /// Remove from global scope
        #[arg(short, long)]
        global: bool,
        /// Uninstall all personas
        #[arg(short, long)]
        all: bool,
    },
    /// List available and installed personas
    List {
        /// Show only installed
        #[arg(long)]
        installed: bool,
        /// Show only local (.agents/personas/)
        #[arg(long)]
        local: bool,
        /// Show only global (~/.agents/personas/)
        #[arg(long)]
        global: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new persona (template or AI-generated)
    Create {
        /// Persona name
        name: String,
        /// Generate with AI (provide a description)
        #[arg(long, value_name = "DESC")]
        ai: Option<String>,
        /// Use Codex for generation
        #[arg(long, value_name = "DESC")]
        codex: Option<String>,
        /// Use Claude for generation
        #[arg(long, value_name = "DESC")]
        claude: Option<String>,
        /// Use Gemini for generation
        #[arg(long, value_name = "DESC")]
        gemini: Option<String>,
        /// Use OpenCode for generation
        #[arg(long, value_name = "DESC")]
        opencode: Option<String>,
    },
    /// Show persona content (reads the markdown file)
    Show {
        /// Persona name
        name: String,
    },
    /// Show the resolved file path of a persona
    Which {
        /// Persona name
        name: String,
    },
    /// Ask a persona to review code or answer a question
    #[command(trailing_var_arg = true)]
    Review {
        /// Persona name
        name: String,
        /// Use Codex for review
        #[arg(long)]
        codex: bool,
        /// Use Claude for review
        #[arg(long)]
        claude: bool,
        /// Use Gemini for review
        #[arg(long)]
        gemini: bool,
        /// Use OpenCode for review
        #[arg(long)]
        opencode: bool,
        /// Review staged changes only
        #[arg(long)]
        staged: bool,
        /// Base branch for diff (e.g., main)
        #[arg(long)]
        base: Option<String>,
        /// Save review output to file
        #[arg(short, long)]
        output: Option<String>,
        /// Custom prompt (skips git diff, asks the persona directly)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
}

pub fn execute(action: PersonaAction) -> Result<()> {
    match action {
        PersonaAction::Install {
            name,
            global,
            force,
            all,
            from,
        } => install(name, global, force, all, from),
        PersonaAction::Uninstall { name, global, all } => {
            if all {
                uninstall_all(global)
            } else {
                let name = name.context("Persona name required (or use --all)")?;
                uninstall(&name, global)
            }
        }
        PersonaAction::List {
            installed,
            local,
            global,
            json,
        } => list(installed, local, global, json),
        PersonaAction::Create {
            name,
            ai,
            codex,
            claude,
            gemini,
            opencode,
        } => create(&name, ai, codex, claude, gemini, opencode),
        PersonaAction::Show { name } => show(&name),
        PersonaAction::Which { name } => which(&name),
        PersonaAction::Review {
            name,
            prompt,
            codex,
            claude,
            gemini,
            opencode,
            staged,
            base,
            output,
        } => {
            let custom_prompt = if prompt.is_empty() {
                None
            } else {
                Some(prompt.join(" "))
            };
            review(
                &name,
                custom_prompt,
                codex,
                claude,
                gemini,
                opencode,
                staged,
                base,
                output,
            )
        }
    }
}

fn install(
    name: Option<String>,
    global: bool,
    force: bool,
    all: bool,
    from: Option<String>,
) -> Result<()> {
    if let Some(spec_str) = from {
        return install_remote(&spec_str, global, force);
    }

    let source_dir = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .context("No personas found. Install from a skills repo or set AGT_DIR.")?;
    let persona_lib = config::persona_library(&source_dir);

    if all {
        return install_all(&persona_lib, global, force);
    }

    let name = name.context("Persona name required (or use --all / --from)")?;
    util::validate_name(&name)?;

    let persona_path = find_in_library(&persona_lib, &name)?;

    let target_dir = if global {
        config::global_persona_target()
    } else {
        config::local_persona_target()
    };

    fs::create_dir_all(&target_dir)?;
    let link_path = target_dir.join(&name);

    install_single_local_persona_link(&persona_path, &link_path, force, &name)?;

    let scope = if global { "global" } else { "local" };
    ui::success(&format!("Installed persona '{}' ({})", name, scope));
    post_persona_install();
    Ok(())
}

fn install_single_local_persona_link(
    persona_path: &Path,
    link_path: &Path,
    force: bool,
    name: &str,
) -> Result<()> {
    install_single_local_persona_link_with(
        persona_path,
        link_path,
        force,
        name,
        util::replace_symlink_transactionally,
    )
}

fn install_single_local_persona_link_with<R>(
    persona_path: &Path,
    link_path: &Path,
    force: bool,
    name: &str,
    mut replace: R,
) -> Result<()>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    if !force {
        util::ensure_target_clear(link_path, false, name)?;
    }
    create_local_persona_link_with(persona_path, link_path, force, &mut replace)
}

fn create_local_persona_link_with<R>(
    persona_path: &Path,
    link_path: &Path,
    force: bool,
    replace: &mut R,
) -> Result<()>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    if force {
        replace(persona_path, link_path)
    } else {
        symlink(persona_path, link_path).map_err(anyhow::Error::from)
    }
    .with_context(|| {
        format!(
            "Failed to create persona symlink: {} -> {}",
            link_path.display(),
            persona_path.display()
        )
    })
}

/// Find a persona in the library — handles both directories and .md files
fn find_in_library(persona_lib: &Path, name: &str) -> Result<PathBuf> {
    // Check exact directory
    let as_dir = persona_lib.join(name);
    if as_dir.is_dir() {
        return Ok(as_dir);
    }
    // Check .md file
    let as_md = persona_lib.join(format!("{}.md", name));
    if as_md.exists() {
        return Ok(as_md);
    }
    // Check subdirectories (e.g., review/security-reviewer)
    if let Ok(entries) = fs::read_dir(persona_lib) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let sub_dir = path.join(name);
                if sub_dir.is_dir() {
                    return Ok(sub_dir);
                }
                let sub_md = path.join(format!("{}.md", name));
                if sub_md.exists() {
                    return Ok(sub_md);
                }
            }
        }
    }
    bail!(
        "Persona '{}' not found in library: {}",
        name,
        persona_lib.display()
    )
}

/// Install all personas from the library
fn install_all(persona_lib: &Path, global: bool, force: bool) -> Result<()> {
    let target_dir = if global {
        config::global_persona_target()
    } else {
        config::local_persona_target()
    };
    fs::create_dir_all(&target_dir)?;

    let count =
        install_all_from_library_with(persona_lib, &target_dir, force, |source, destination| {
            util::replace_symlink_transactionally(source, destination)
        })?;

    let scope = if global { "global" } else { "local" };
    ui::success(&format!("Installed {} personas ({})", count, scope));
    if count > 0 {
        post_persona_install();
    }
    Ok(())
}

fn install_all_from_library_with<R>(
    persona_lib: &Path,
    target_dir: &Path,
    force: bool,
    mut replace: R,
) -> Result<usize>
where
    R: FnMut(&Path, &Path) -> Result<()>,
{
    let mut count = 0;
    for entry in fs::read_dir(persona_lib)?.flatten() {
        let path = entry.path();
        let raw_name = entry.file_name().to_string_lossy().to_string();

        // Skip README and hidden files
        if raw_name.starts_with('.') || raw_name == "README.md" {
            continue;
        }

        let name = raw_name
            .strip_suffix(".md")
            .unwrap_or(&raw_name)
            .to_string();
        let link_path = target_dir.join(&name);

        if !force && (link_path.exists() || link_path.is_symlink()) {
            ui::warn(&format!(
                "Skipping '{}' (already exists, use --force)",
                name
            ));
            continue;
        }

        create_local_persona_link_with(&path, &link_path, force, &mut replace)?;
        count += 1;
    }
    Ok(count)
}

fn install_remote(spec_str: &str, global: bool, force: bool) -> Result<()> {
    let spec = remote::parse_spec(spec_str)?;

    // Repo-level: owner/repo with no path — discover and install all personas
    if spec.path.is_empty() {
        return install_remote_repo(&spec, global, force);
    }
    remote::validate_source_path(&spec.path)?;

    ui::info(&format!("Downloading {}...", spec));

    // Try single-file download first (personas are often a single .md)
    let file_spec = remote::RemoteSpec {
        owner: spec.owner.clone(),
        repo: spec.repo.clone(),
        path: format!("{}/PERSONA.md", spec.path),
        git_ref: spec.git_ref.clone(),
    };

    let persona_name = spec
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&spec.path)
        .to_string();

    util::validate_name(&persona_name)?;

    let target_dir = if global {
        config::global_persona_target()
    } else {
        config::local_persona_target()
    };

    fs::create_dir_all(&target_dir)?;
    let dest = target_dir.join(&persona_name);

    ensure_remote_persona_destination(&dest, force, &persona_name)?;

    // Try fetching as a directory (tarball)
    match remote::fetch_dir(&spec) {
        Ok((_tmp_dir, source_path)) => {
            replace_remote_persona_path(&source_path, &dest, &spec)?;
        }
        Err(_) => {
            // Fallback: try single PERSONA.md file
            let data = remote::fetch_file(&file_spec)
                .context(format!("Failed to download persona '{}'", persona_name))?;

            replace_remote_persona_bytes(&data, &dest, &spec)?;
        }
    }

    let scope = if global { "global" } else { "local" };
    ui::success(&format!(
        "Installed remote persona '{}' ({}) from {}",
        persona_name, scope, spec
    ));
    post_persona_install();
    Ok(())
}

fn ensure_remote_persona_destination(dest: &Path, force: bool, persona_name: &str) -> Result<()> {
    // A forced remote refresh keeps the live persona in place until its replacement,
    // including metadata, has been staged successfully.
    if force {
        Ok(())
    } else {
        util::ensure_target_clear(dest, false, persona_name)
    }
}

fn replace_remote_persona_path(
    source: &Path,
    dest: &Path,
    spec: &remote::RemoteSpec,
) -> Result<()> {
    replace_remote_persona_path_with(source, dest, |staged| remote::write_metadata(staged, spec))
}

fn replace_remote_persona_path_with<F>(source: &Path, dest: &Path, prepare: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if source.is_dir() {
        return util::replace_dir_transactionally(source, dest, prepare);
    }

    let source_root = tempfile::TempDir::new().context("Failed to stage remote persona file")?;
    fs::copy(source, source_root.path().join("PERSONA.md"))
        .context("Failed to stage remote persona file")?;
    util::replace_dir_transactionally(source_root.path(), dest, prepare)
}

fn replace_remote_persona_bytes(data: &[u8], dest: &Path, spec: &remote::RemoteSpec) -> Result<()> {
    replace_remote_persona_bytes_with(data, dest, |staged| remote::write_metadata(staged, spec))
}

fn replace_remote_persona_bytes_with<F>(data: &[u8], dest: &Path, prepare: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    let source_root = tempfile::TempDir::new().context("Failed to stage remote persona file")?;
    fs::write(source_root.path().join("PERSONA.md"), data)
        .context("Failed to stage remote persona file")?;
    util::replace_dir_transactionally(source_root.path(), dest, prepare)
}

fn install_remote_repo(spec: &remote::RemoteSpec, global: bool, force: bool) -> Result<()> {
    ui::info(&format!(
        "Downloading {}/{}@{}...",
        spec.owner, spec.repo, spec.git_ref
    ));
    let (_tmp_dir, repo_root) = remote::fetch_dir(spec)?;

    // Look for personas/ directory in the repo
    let persona_dir = repo_root.join("personas");
    if !persona_dir.is_dir() {
        bail!(
            "No personas/ directory found in {}/{}",
            spec.owner,
            spec.repo
        );
    }

    // Discover all personas in the repo
    let mut available: Vec<(String, String, PathBuf, String)> = Vec::new(); // (name, role, path, raw_name)
    for entry in fs::read_dir(&persona_dir)?.flatten() {
        let path = entry.path();
        let raw_name = entry.file_name().to_string_lossy().to_string();
        if raw_name.starts_with('.') || raw_name == "README.md" {
            continue;
        }

        let is_persona = if path.is_dir() {
            path.join("PERSONA.md").exists()
                || fs::read_dir(&path)
                    .map(|rd| {
                        rd.flatten()
                            .any(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    })
                    .unwrap_or(false)
        } else {
            path.extension().is_some_and(|e| e == "md")
        };

        if !is_persona {
            continue;
        }

        let name = raw_name
            .strip_suffix(".md")
            .unwrap_or(&raw_name)
            .to_string();
        let (role, _, _) = if path.is_dir() {
            read_persona_info(&path)
        } else {
            read_persona_info_from_file(&path)
        };
        available.push((name, role, path, raw_name));
    }

    if available.is_empty() {
        bail!("No personas found in {}/{}", spec.owner, spec.repo);
    }

    ui::info(&format!("Found {} personas", available.len()));

    // Interactive selection if TTY
    let is_tty = console::Term::stderr().is_term();
    let installed_names = installed_persona_names(&if global {
        config::global_persona_target()
    } else {
        config::local_persona_target()
    });

    let names_to_install: Vec<String> = if is_tty {
        let persona_list: Vec<(String, String)> = available
            .iter()
            .map(|(name, role, _, _)| (name.clone(), role.clone()))
            .collect();

        match ui::interactive::select_personas(&persona_list, &installed_names, global)? {
            Some(names) => names,
            None => {
                ui::info("Installation cancelled.");
                return Ok(());
            }
        }
    } else {
        available
            .iter()
            .map(|(name, _, _, _)| name.clone())
            .collect()
    };

    let target_dir = if global {
        config::global_persona_target()
    } else {
        config::local_persona_target()
    };
    fs::create_dir_all(&target_dir)?;

    let scope = if global { "global" } else { "local" };
    let mut installed = 0;
    let mut skipped = 0;

    for (name, _role, path, raw_name) in &available {
        if !names_to_install.contains(name) {
            continue;
        }

        let dest = target_dir.join(name);

        if (dest.exists() || dest.is_symlink()) && !force {
            skipped += 1;
            continue;
        }

        let persona_spec = remote::RemoteSpec {
            owner: spec.owner.clone(),
            repo: spec.repo.clone(),
            path: format!("personas/{}", raw_name),
            git_ref: spec.git_ref.clone(),
        };
        replace_remote_persona_path(path, &dest, &persona_spec)?;
        ui::success(&format!("Installed persona '{}' ({})", name, scope));
        installed += 1;
    }

    ui::success(&format!(
        "Done: {} personas installed, {} skipped from {}/{}",
        installed, skipped, spec.owner, spec.repo
    ));
    if installed > 0 {
        post_persona_install();
    }
    Ok(())
}

fn uninstall(name: &str, global: bool) -> Result<()> {
    let scope = uninstall_from_roots(
        name,
        global,
        &config::local_persona_target(),
        &config::global_persona_target(),
    )?;

    ui::success(&format!("Uninstalled persona '{}' ({})", name, scope));
    Ok(())
}

fn uninstall_from_roots(
    name: &str,
    global: bool,
    local_dir: &Path,
    global_dir: &Path,
) -> Result<&'static str> {
    util::validate_name(name)?;

    let (dir, path, scope) = if global {
        (
            global_dir,
            find_installed_persona_for_removal(name, global_dir)?,
            "global",
        )
    } else {
        let local = find_installed_persona_for_removal(name, local_dir)?;
        if local.is_none() && find_installed_persona_for_removal(name, global_dir)?.is_some() {
            bail!(
                "Persona '{}' is installed globally; retry with --global to uninstall it",
                name
            );
        }
        (local_dir, local, "local")
    };

    let path = path.context(format!(
        "Persona '{}' is not installed in {} scope",
        name, scope
    ))?;
    remove_persona_entry(dir, &path)?;
    Ok(scope)
}

fn uninstall_all(global: bool) -> Result<()> {
    let dir = if global {
        config::global_persona_target()
    } else {
        config::local_persona_target()
    };

    let Some(count) = remove_all_persona_entries(&dir)? else {
        ui::info("No personas installed.");
        return Ok(());
    };

    let scope = if global { "global" } else { "local" };
    ui::success(&format!("Uninstalled {} personas ({})", count, scope));
    Ok(())
}

/// Reject persona stores whose `.agents` parent or `personas` root is a symlink.
/// These are the mutable path components owned by persona installation; following
/// either during uninstall could redirect recursive deletion outside the store.
fn ensure_persona_store_safe(dir: &Path) -> Result<()> {
    for component in [dir.parent(), Some(dir)].into_iter().flatten() {
        if component.is_symlink() {
            bail!(
                "Refusing to uninstall through symlinked persona store path: {}",
                component.display()
            );
        }
    }
    Ok(())
}

fn find_installed_persona_for_removal(name: &str, dir: &Path) -> Result<Option<PathBuf>> {
    ensure_persona_store_safe(dir)?;
    Ok(find_installed_persona(name, dir))
}

fn remove_all_persona_entries(dir: &Path) -> Result<Option<usize>> {
    ensure_persona_store_safe(dir)?;
    if !dir.is_dir() {
        return Ok(None);
    }

    let mut count = 0;
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        remove_persona_entry(dir, &entry.path())?;
        count += 1;
    }
    Ok(Some(count))
}

fn remove_persona_entry(dir: &Path, path: &Path) -> Result<()> {
    ensure_persona_store_safe(dir)?;
    let relative = path.strip_prefix(dir).with_context(|| {
        format!(
            "Refusing to remove path outside persona store: {}",
            path.display()
        )
    })?;
    let mut components = relative.components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        bail!(
            "Refusing to remove malformed path outside persona store: {}",
            path.display()
        );
    }

    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn find_installed_persona(name: &str, dir: &Path) -> Option<PathBuf> {
    // Check exact name (directory or file)
    let exact = dir.join(name);
    if exact.exists() || exact.is_symlink() {
        return Some(exact);
    }
    // Check with .md extension
    let with_md = dir.join(format!("{}.md", name));
    if with_md.exists() || with_md.is_symlink() {
        return Some(with_md);
    }
    None
}

fn list(installed: bool, local: bool, global: bool, json: bool) -> Result<()> {
    let local_dir = config::local_persona_target();
    let global_dir = config::global_persona_target();
    let local_installed = installed_persona_names(&local_dir);
    let global_installed = installed_persona_names(&global_dir);

    let mut entries: Vec<serde_json::Value> = Vec::new();

    // If only showing installed
    if installed || local || global {
        if (installed || local) && local_dir.is_dir() {
            list_personas_in_dir(&local_dir, "local", &mut entries)?;
        }
        if (installed || global) && global_dir.is_dir() {
            list_personas_in_dir(&global_dir, "global", &mut entries)?;
        }
        if json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
            return Ok(());
        }
        if entries.is_empty() {
            ui::info("No installed personas found.");
            return Ok(());
        }
        let mut table = ui::table::new_table();
        table.set_header(["Name", "Scope", "Role"]);
        for entry in &entries {
            let name = entry["name"].as_str().unwrap_or("");
            let scope = entry["scope"].as_str().unwrap_or("");
            let role = entry["role"].as_str().unwrap_or("");
            ui::table::add_row(&mut table, &[name, scope, role]);
        }
        println!("{table}");
        return Ok(());
    }

    // Default: show all library personas with install status
    // Collect library entries
    if let Some(source_dir) = config::find_source_dir().or_else(config::find_cwd_source_dir) {
        let lib_dir = config::persona_library(&source_dir);
        if lib_dir.is_dir() {
            list_personas_in_dir(&lib_dir, "library", &mut entries)?;
        }
    }

    // Also add installed-only personas not in library
    if local_dir.is_dir() {
        list_personas_in_dir(&local_dir, "local", &mut entries)?;
    }
    if global_dir.is_dir() {
        list_personas_in_dir(&global_dir, "global", &mut entries)?;
    }

    // Deduplicate by name (library entries first)
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| {
        let name = e["name"].as_str().unwrap_or("").to_string();
        seen.insert(name)
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        ui::info("No personas found.");
        return Ok(());
    }

    // Group by type
    use std::collections::BTreeMap;

    let mut by_type: BTreeMap<String, Vec<&serde_json::Value>> = BTreeMap::new();
    for entry in &entries {
        let kind = entry["type"].as_str().unwrap_or("other").to_string();
        let kind = if kind.is_empty() {
            "other".to_string()
        } else {
            kind
        };
        by_type.entry(kind).or_default().push(entry);
    }

    let total = entries.len();
    let total_installed: usize = entries
        .iter()
        .filter(|e| {
            let name = e["name"].as_str().unwrap_or("");
            local_installed.contains(&name.to_string())
                || global_installed.contains(&name.to_string())
        })
        .count();

    ui::section("Available Personas");

    for (kind, personas) in &by_type {
        ui::subsection(&format!("{}/", kind));

        let mut table = ui::table::new_table();
        for entry in personas {
            let name = entry["name"].as_str().unwrap_or("");
            let role = entry["role"].as_str().unwrap_or("");

            let status = if local_installed.contains(&name.to_string()) {
                "L".green().bold().to_string()
            } else if global_installed.contains(&name.to_string()) {
                "G".blue().bold().to_string()
            } else {
                "○".dimmed().to_string()
            };

            let role_styled = role.dimmed().to_string();
            ui::table::add_row(&mut table, &[status.as_str(), name, role_styled.as_str()]);
        }
        if !personas.is_empty() {
            println!("{table}");
        }
    }

    ui::info(&format!(
        "Total: {} personas, {} installed",
        total, total_installed
    ));

    Ok(())
}

fn create(
    name: &str,
    ai: Option<String>,
    codex: Option<String>,
    claude: Option<String>,
    gemini: Option<String>,
    opencode: Option<String>,
) -> Result<()> {
    util::validate_name(name)?;
    let target_dir = config::local_persona_target();
    fs::create_dir_all(&target_dir)?;

    let persona_dir = target_dir.join(name);
    if persona_dir.exists() {
        bail!(
            "Persona '{}' already exists at {}",
            name,
            persona_dir.display()
        );
    }

    // Determine if AI generation is requested
    let ai_desc = ai
        .or(codex.clone())
        .or(claude.clone())
        .or(gemini.clone())
        .or(opencode.clone());

    let cli_override = if codex.is_some() {
        Some(llm::LlmCli::Codex)
    } else if claude.is_some() {
        Some(llm::LlmCli::Claude)
    } else if opencode.is_some() {
        Some(llm::LlmCli::OpenCode)
    } else if gemini.is_some() {
        Some(llm::LlmCli::Gemini)
    } else {
        None
    };

    let content = if let Some(desc) = ai_desc {
        generate_persona(name, &desc, cli_override)?
    } else {
        default_persona_template(name)
    };

    fs::create_dir_all(&persona_dir)?;
    fs::write(persona_dir.join("PERSONA.md"), content)?;

    ui::success(&format!(
        "Created persona '{}' at {}",
        name,
        persona_dir.display()
    ));
    Ok(())
}

fn show(name: &str) -> Result<()> {
    util::validate_name(name)?;
    let path = find_persona(name)?;
    let persona_md = find_persona_md(&path)?;
    let content = fs::read_to_string(&persona_md)?;

    let (fm, body) = frontmatter::parse(&content)?;

    let mut table = ui::table::new_table();
    table.set_header(["Field", "Value"]);
    if let Some(n) = &fm.name {
        ui::table::add_row(&mut table, &["Name", n]);
    }
    if let Some(role) = &fm.role {
        ui::table::add_row(&mut table, &["Role", role]);
    }
    if let Some(domain) = &fm.domain {
        ui::table::add_row(&mut table, &["Domain", domain]);
    }
    if let Some(kind) = &fm.kind {
        ui::table::add_row(&mut table, &["Type", kind]);
    }
    if let Some(desc) = &fm.description {
        ui::table::add_row(&mut table, &["Description", desc]);
    }
    if let Some(tags) = &fm.tags {
        ui::table::add_row(&mut table, &["Tags", &tags.join(", ")]);
    }
    println!("{table}");
    println!();
    println!("{}", body);

    Ok(())
}

fn which(name: &str) -> Result<()> {
    util::validate_name(name)?;
    let path = find_persona(name)?;
    let resolved = fs::canonicalize(&path).unwrap_or(path);
    println!("{}", resolved.display());
    Ok(())
}

// CLI flag fan-out: each argument maps 1:1 to a `persona review` option.
#[allow(clippy::too_many_arguments)]
fn review(
    name: &str,
    custom_prompt: Option<String>,
    use_codex: bool,
    use_claude: bool,
    use_gemini: bool,
    use_opencode: bool,
    staged: bool,
    base: Option<String>,
    output: Option<String>,
) -> Result<()> {
    util::validate_name(name)?;
    let persona_path = find_persona(name)?;
    let persona_md = find_persona_md(&persona_path)?;
    let persona_content = fs::read_to_string(&persona_md)?;

    // Determine LLM
    let cli = if use_codex {
        llm::LlmCli::Codex
    } else if use_claude {
        llm::LlmCli::Claude
    } else if use_opencode {
        llm::LlmCli::OpenCode
    } else if use_gemini {
        llm::LlmCli::Gemini
    } else {
        llm::detect()
            .context("No LLM CLI found. Install codex, claude, opencode, gemini, or ollama.")?
    };

    // Build prompt: custom prompt mode vs diff review mode
    let full_prompt = if let Some(user_prompt) = custom_prompt {
        ui::info(&format!("Asking {} using persona '{}'...", cli, name));
        format!(
            "You are acting as the following persona:\n\n{}\n\n\
             User question:\n{}",
            persona_content, user_prompt
        )
    } else {
        let diff = get_diff(staged, base.as_deref())?;
        if diff.trim().is_empty() {
            ui::warn("No changes to review.");
            return Ok(());
        }
        ui::info(&format!(
            "Reviewing with {} using persona '{}'...",
            cli, name
        ));
        format!(
            "You are acting as the following persona:\n\n{}\n\n\
             Review the following code changes and provide feedback:\n\n\
             ```diff\n{}\n```\n\n\
             Provide a structured review with: issues found, suggestions, and an overall assessment.",
            persona_content, diff
        )
    };

    let result = llm::invoke(cli, &full_prompt)?;

    if let Some(output_path) = output {
        fs::write(&output_path, &result)?;
        ui::success(&format!("Review saved to {}", output_path));
    } else {
        println!("{}", result);
    }

    Ok(())
}

// --- Helpers ---

fn find_persona(name: &str) -> Result<PathBuf> {
    // Check local (dir or .md)
    let local_dir = config::local_persona_target().join(name);
    let local_md = config::local_persona_target().join(format!("{}.md", name));
    if local_dir.exists() {
        return Ok(local_dir);
    }
    if local_md.exists() {
        return Ok(local_md);
    }

    // Check global (dir or .md)
    let global_dir = config::global_persona_target().join(name);
    let global_md = config::global_persona_target().join(format!("{}.md", name));
    if global_dir.exists() {
        return Ok(global_dir);
    }
    if global_md.exists() {
        return Ok(global_md);
    }

    // Check library (dir or .md)
    if let Some(source_dir) = config::find_source_dir().or_else(config::find_cwd_source_dir) {
        let lib = config::persona_library(&source_dir);
        let lib_dir = lib.join(name);
        let lib_md = lib.join(format!("{}.md", name));
        if lib_dir.exists() {
            return Ok(lib_dir);
        }
        if lib_md.exists() {
            return Ok(lib_md);
        }
    }

    bail!("Persona '{}' not found", name);
}

/// Find the persona markdown content. Handles both:
/// - Directory with PERSONA.md inside
/// - Single .md file
fn find_persona_md(path: &Path) -> Result<PathBuf> {
    // If path is already a .md file
    if path.is_file() && path.extension().is_some_and(|e| e == "md") {
        return Ok(path.to_path_buf());
    }

    // Directory: check PERSONA.md first, then any .md
    let persona_md = path.join("PERSONA.md");
    if persona_md.exists() {
        return Ok(persona_md);
    }
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "md") {
                return Ok(entry.path());
            }
        }
    }
    bail!("No PERSONA.md found in {}", path.display());
}

fn installed_persona_names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            // Strip .md extension for matching
            let clean = name.strip_suffix(".md").unwrap_or(&name).to_string();
            names.push(clean);
        }
    }
    names
}

fn list_personas_in_dir(
    dir: &Path,
    scope: &str,
    entries: &mut Vec<serde_json::Value>,
) -> Result<()> {
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            let raw_name = entry.file_name().to_string_lossy().to_string();
            if raw_name.starts_with('.') || raw_name == "README.md" {
                continue;
            }

            // Handle both directories and .md files
            let (name, role, domain, kind) = if path.is_dir() {
                let (role, domain, kind) = read_persona_info(&path);
                (raw_name, role, domain, kind)
            } else if path.extension().is_some_and(|e| e == "md") {
                let persona_name = raw_name
                    .strip_suffix(".md")
                    .unwrap_or(&raw_name)
                    .to_string();
                let (role, domain, kind) = read_persona_info_from_file(&path);
                (persona_name, role, domain, kind)
            } else {
                continue;
            };

            let is_remote = if path.is_dir() {
                path.join(".remote-source").exists()
            } else {
                false
            };

            entries.push(serde_json::json!({
                "name": name,
                "scope": scope,
                "role": role,
                "domain": domain,
                "type": kind,
                "remote": is_remote,
            }));
        }
    }
    Ok(())
}

fn read_persona_info(path: &Path) -> (String, String, String) {
    let persona_md = path.join("PERSONA.md");
    if let Ok(content) = fs::read_to_string(persona_md) {
        return extract_persona_fields(&content);
    }
    // Try any .md in dir
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "md") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    return extract_persona_fields(&content);
                }
            }
        }
    }
    (String::new(), String::new(), String::new())
}

fn read_persona_info_from_file(path: &Path) -> (String, String, String) {
    if let Ok(content) = fs::read_to_string(path) {
        return extract_persona_fields(&content);
    }
    (String::new(), String::new(), String::new())
}

fn extract_persona_fields(content: &str) -> (String, String, String) {
    let role = frontmatter::get_field(content, "role").unwrap_or_default();
    let domain = frontmatter::get_field(content, "domain").unwrap_or_default();
    let kind = frontmatter::get_field(content, "type").unwrap_or_default();
    (role, domain, kind)
}

fn get_diff(staged: bool, base: Option<&str>) -> Result<String> {
    let output = if staged {
        std::process::Command::new("git")
            .args(["diff", "--cached"])
            .output()
            .context("Failed to run git diff")?
    } else if let Some(base_branch) = base {
        // Validate base branch to prevent argument injection
        if base_branch.starts_with('-') {
            bail!("Invalid base branch name: {}", base_branch);
        }
        std::process::Command::new("git")
            .args(["diff", &format!("{}...HEAD", base_branch)])
            .output()
            .context("Failed to run git diff")?
    } else {
        std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .output()
            .context("Failed to run git diff")?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr);
    }

    Ok(String::from_utf8(output.stdout)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

fn generate_persona(name: &str, desc: &str, cli_override: Option<llm::LlmCli>) -> Result<String> {
    let cli = cli_override
        .or_else(llm::detect)
        .context("No LLM CLI found for persona generation")?;

    ui::info(&format!("Generating persona with {}...", cli));

    let prompt = format!(
        "Create a code review persona in YAML frontmatter + markdown format.\n\n\
         Name: {}\nDescription: {}\n\n\
         Use this exact format:\n\
         ---\nname: {}\nrole: \"<role title>\"\ndomain: <domain>\n\
         type: review\ntags: [<tag1>, <tag2>]\n---\n\n\
         # <Title>\n\n<Detailed persona instructions for code review>\n\n\
         Output ONLY the persona file content, no explanation.",
        name, desc, name
    );

    llm::invoke(cli, &prompt)
}

fn default_persona_template(name: &str) -> String {
    format!(
        "---\nname: {}\nrole: \"Code Reviewer\"\ndomain: general\n\
         type: review\ntags: [review]\n---\n\n\
         # {}\n\nReview code for correctness, readability, and best practices.\n",
        name, name
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_remote_persona_destination, install_all_from_library_with,
        install_single_local_persona_link, install_single_local_persona_link_with,
        post_persona_install, remove_all_persona_entries, remove_persona_entry,
        replace_remote_persona_bytes_with, replace_remote_persona_path_with, uninstall_from_roots,
    };
    use anyhow::bail;
    use std::fs;

    #[test]
    fn forced_local_persona_install_replaces_existing_entry_with_link() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("installed");
        fs::write(&source, "new persona").unwrap();
        fs::write(&destination, "old persona").unwrap();

        install_single_local_persona_link(&source, &destination, true, "installed").unwrap();

        assert!(destination.is_symlink());
        assert_eq!(fs::read_link(destination).unwrap(), source);
    }

    #[test]
    fn forced_single_persona_propagates_candidate_failure_and_preserves_old_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("installed");
        fs::write(&source, "new persona").unwrap();
        fs::write(&destination, "old persona").unwrap();

        let error = install_single_local_persona_link_with(
            &source,
            &destination,
            true,
            "installed",
            |_, _| bail!("injected candidate failure"),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected candidate failure"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old persona");
    }

    #[test]
    fn forced_persona_all_propagates_activation_failure_and_preserves_old_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let library = temp.path().join("library");
        let target = temp.path().join("target");
        fs::create_dir_all(&library).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(library.join("reviewer.md"), "new persona").unwrap();
        let destination = target.join("reviewer");
        fs::write(&destination, "old persona").unwrap();

        let error = install_all_from_library_with(&library, &target, true, |_, _| {
            bail!("injected activation failure")
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected activation failure"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "old persona");
    }

    fn existing_persona(root: &std::path::Path) -> std::path::PathBuf {
        let destination = root.join("installed");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("PERSONA.md"), "old bytes").unwrap();
        fs::write(destination.join(".remote-source"), "old metadata").unwrap();
        destination
    }

    #[test]
    fn direct_remote_force_activates_content_and_metadata_together() {
        let temp = tempfile::TempDir::new().unwrap();
        let destination = existing_persona(temp.path());

        replace_remote_persona_bytes_with(b"new bytes", &destination, |staged| {
            assert_eq!(
                fs::read_to_string(staged.join("PERSONA.md")).unwrap(),
                "new bytes"
            );
            assert_eq!(
                fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
                "old bytes"
            );
            fs::write(staged.join(".remote-source"), "new metadata")?;
            Ok(())
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
            "new bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "new metadata"
        );
    }

    #[test]
    fn direct_remote_prepare_failure_preserves_existing_persona() {
        let temp = tempfile::TempDir::new().unwrap();
        let destination = existing_persona(temp.path());

        let error = replace_remote_persona_bytes_with(b"new bytes", &destination, |_| {
            bail!("injected metadata failure")
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected metadata failure"));
        assert_eq!(
            fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
            "old bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "old metadata"
        );
    }

    #[test]
    fn repo_remote_force_activates_content_and_metadata_together() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("repo-persona");
        let destination = existing_persona(temp.path());
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("PERSONA.md"), "repo bytes").unwrap();

        replace_remote_persona_path_with(&source, &destination, |staged| {
            assert_eq!(
                fs::read_to_string(staged.join("PERSONA.md")).unwrap(),
                "repo bytes"
            );
            assert_eq!(
                fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
                "old bytes"
            );
            fs::write(staged.join(".remote-source"), "repo metadata")?;
            Ok(())
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
            "repo bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "repo metadata"
        );
    }

    #[test]
    fn repo_remote_copy_failure_preserves_existing_persona() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing_source = temp.path().join("missing-persona.md");
        let destination = existing_persona(temp.path());

        let error = replace_remote_persona_path_with(&missing_source, &destination, |_| Ok(()))
            .unwrap_err();

        assert!(format!("{error:#}").contains("Failed to stage remote persona file"));
        assert_eq!(
            fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
            "old bytes"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".remote-source")).unwrap(),
            "old metadata"
        );
    }

    #[test]
    fn direct_remote_no_force_conflict_is_unchanged() {
        let temp = tempfile::TempDir::new().unwrap();
        let destination = existing_persona(temp.path());

        let error =
            ensure_remote_persona_destination(&destination, false, "installed").unwrap_err();

        assert!(format!("{error:#}").contains("Use --force to overwrite"));
        assert_eq!(
            fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
            "old bytes"
        );
        ensure_remote_persona_destination(&destination, true, "installed").unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("PERSONA.md")).unwrap(),
            "old bytes"
        );
    }

    #[test]
    fn local_uninstall_reports_global_match_without_mutating_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let local = temp.path().join("local/.agents/personas");
        let global = temp.path().join("global/.agents/personas");
        fs::create_dir_all(&global).unwrap();
        let global_persona = global.join("reviewer.md");
        fs::write(&global_persona, "global sentinel").unwrap();

        let error = uninstall_from_roots("reviewer", false, &local, &global).unwrap_err();

        assert!(error.to_string().contains("retry with --global"));
        assert_eq!(
            fs::read_to_string(global_persona).unwrap(),
            "global sentinel"
        );
    }

    #[test]
    fn local_uninstall_removes_only_local_match_when_both_scopes_match() {
        let temp = tempfile::TempDir::new().unwrap();
        let local = temp.path().join("local/.agents/personas");
        let global = temp.path().join("global/.agents/personas");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&global).unwrap();
        let local_persona = local.join("reviewer.md");
        let global_persona = global.join("reviewer.md");
        fs::write(&local_persona, "local sentinel").unwrap();
        fs::write(&global_persona, "global sentinel").unwrap();

        assert_eq!(
            uninstall_from_roots("reviewer", false, &local, &global).unwrap(),
            "local"
        );

        assert!(!local_persona.exists());
        assert_eq!(
            fs::read_to_string(global_persona).unwrap(),
            "global sentinel"
        );
    }

    #[test]
    fn global_uninstall_removes_only_global_match() {
        let temp = tempfile::TempDir::new().unwrap();
        let local = temp.path().join("local/.agents/personas");
        let global = temp.path().join("global/.agents/personas");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&global).unwrap();
        let local_persona = local.join("reviewer.md");
        let global_persona = global.join("reviewer.md");
        fs::write(&local_persona, "local sentinel").unwrap();
        fs::write(&global_persona, "global sentinel").unwrap();

        assert_eq!(
            uninstall_from_roots("reviewer", true, &local, &global).unwrap(),
            "global"
        );

        assert_eq!(fs::read_to_string(local_persona).unwrap(), "local sentinel");
        assert!(!global_persona.exists());
    }

    #[cfg(unix)]
    #[test]
    fn single_uninstall_rejects_symlinked_store_root_without_touching_outside_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.md");
        fs::write(&sentinel, "outside sentinel").unwrap();

        let agents = temp.path().join("home/.agents");
        fs::create_dir_all(&agents).unwrap();
        let store = agents.join("personas");
        std::os::unix::fs::symlink(&outside, &store).unwrap();

        let error = remove_persona_entry(&store, &store.join("sentinel.md")).unwrap_err();
        assert!(error.to_string().contains("symlinked persona store path"));
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn single_uninstall_rejects_symlinked_store_parent_without_touching_outside_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let outside = temp.path().join("outside/.agents/personas");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.md");
        fs::write(&sentinel, "outside sentinel").unwrap();

        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(temp.path().join("outside/.agents"), home.join(".agents"))
            .unwrap();
        let store = home.join(".agents/personas");

        let error = remove_persona_entry(&store, &store.join("sentinel.md")).unwrap_err();
        assert!(error.to_string().contains("symlinked persona store path"));
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn single_uninstall_removes_only_store_link_not_outside_target() {
        let temp = tempfile::TempDir::new().unwrap();
        let store = temp.path().join("home/.agents/personas");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&store).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.md");
        fs::write(&sentinel, "outside sentinel").unwrap();
        let installed = store.join("installed");
        std::os::unix::fs::symlink(&sentinel, &installed).unwrap();

        remove_persona_entry(&store, &installed).unwrap();

        assert!(fs::symlink_metadata(installed).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_all_rejects_symlinked_store_root_without_touching_outside_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.md");
        fs::write(&sentinel, "outside sentinel").unwrap();

        let agents = temp.path().join("home/.agents");
        fs::create_dir_all(&agents).unwrap();
        let store = agents.join("personas");
        std::os::unix::fs::symlink(&outside, &store).unwrap();

        let error = remove_all_persona_entries(&store).unwrap_err();
        assert!(error.to_string().contains("symlinked persona store path"));
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_all_rejects_symlinked_store_parent_without_touching_outside_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let outside = temp.path().join("outside/.agents/personas");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.md");
        fs::write(&sentinel, "outside sentinel").unwrap();

        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(temp.path().join("outside/.agents"), home.join(".agents"))
            .unwrap();
        let store = home.join(".agents/personas");

        let error = remove_all_persona_entries(&store).unwrap_err();
        assert!(error.to_string().contains("symlinked persona store path"));
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_all_removes_only_store_entries_not_outside_targets() {
        let temp = tempfile::TempDir::new().unwrap();
        let store = temp.path().join("home/.agents/personas");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&store).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.md");
        fs::write(&sentinel, "outside sentinel").unwrap();
        let linked_persona = store.join("linked-persona");
        std::os::unix::fs::symlink(&sentinel, &linked_persona).unwrap();
        let copied_persona = store.join("copied-persona");
        fs::create_dir_all(&copied_persona).unwrap();
        fs::write(copied_persona.join("PERSONA.md"), "inside").unwrap();

        assert_eq!(remove_all_persona_entries(&store).unwrap(), Some(2));

        assert!(fs::symlink_metadata(linked_persona).is_err());
        assert!(fs::symlink_metadata(copied_persona).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[test]
    fn persona_install_does_not_execute_env_discovered_static_index_script() {
        const CHILD_ENV: &str = "AGT_PERSONA_ENV_SCRIPT_CHILD";
        const MARKER_ENV: &str = "AGT_PERSONA_AMBIENT_SCRIPT_MARKER";

        if std::env::var_os(CHILD_ENV).is_some() {
            post_persona_install();
            return;
        }

        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("ambient-source");
        let script_dir = source.join("context/static-index/scripts");
        fs::create_dir_all(&script_dir).unwrap();
        fs::write(
            script_dir.join("static-index.sh"),
            format!("#!/bin/sh\nprintf executed > \"${MARKER_ENV}\"\n"),
        )
        .unwrap();
        let marker = temp.path().join("ambient-script-executed");

        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("cmd::persona::tests::persona_install_does_not_execute_env_discovered_static_index_script")
            .arg("--nocapture")
            .current_dir(temp.path())
            .env(CHILD_ENV, "1")
            .env("AGT_DIR", &source)
            .env(MARKER_ENV, &marker)
            .status()
            .unwrap();

        assert!(status.success());
        assert!(
            !marker.exists(),
            "persona post-install executed an ambient static-index script"
        );
    }

    #[test]
    fn persona_install_does_not_execute_cwd_discovered_static_index_script() {
        const CHILD_ENV: &str = "AGT_PERSONA_CWD_SCRIPT_CHILD";
        const MARKER_ENV: &str = "AGT_PERSONA_AMBIENT_SCRIPT_MARKER";

        if std::env::var_os(CHILD_ENV).is_some() {
            post_persona_install();
            return;
        }

        let source = tempfile::TempDir::new().unwrap();
        let script_dir = source.path().join("context/static-index/scripts");
        fs::create_dir_all(&script_dir).unwrap();
        fs::write(
            script_dir.join("static-index.sh"),
            format!("#!/bin/sh\nprintf executed > \"${MARKER_ENV}\"\n"),
        )
        .unwrap();
        fs::create_dir_all(source.path().join("review/test-persona")).unwrap();
        fs::write(
            source.path().join("review/test-persona/SKILL.md"),
            "---\nname: test-persona\n---\n",
        )
        .unwrap();
        let marker = source.path().join("ambient-script-executed");

        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("cmd::persona::tests::persona_install_does_not_execute_cwd_discovered_static_index_script")
            .arg("--nocapture")
            .current_dir(source.path())
            .env(CHILD_ENV, "1")
            .env_remove("AGT_DIR")
            .env_remove("AGENT_SKILLS_DIR")
            .env(MARKER_ENV, &marker)
            .status()
            .unwrap();

        assert!(status.success());
        assert!(
            !marker.exists(),
            "persona post-install executed a CWD-discovered static-index script"
        );
    }
}

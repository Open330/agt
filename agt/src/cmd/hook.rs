use crate::{config, ui};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Component;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum HookAction {
    /// Install hooks to Claude Code settings
    Install {
        /// Hook name (from library). Omit to install all.
        name: Option<String>,
        /// Force overwrite existing hooks
        #[arg(short, long)]
        force: bool,
    },
    /// Uninstall hooks from Claude Code settings
    Uninstall {
        /// Hook name. Omit to uninstall all.
        name: Option<String>,
    },
    /// List available and installed hooks
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Test a hook by sending a simulated event payload
    Test {
        /// Hook name to test
        name: String,
        /// Event payload JSON (default: minimal test payload)
        #[arg(long)]
        payload: Option<String>,
    },
    /// Start the HTTP hook server
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "9400")]
        port: u16,
    },
    /// Show details of a specific hook
    Show {
        /// Hook name
        name: String,
    },
}

/// Hook types matching Claude Code's hook system
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookType {
    Command,
    Http,
    Prompt,
    Agent,
}

impl std::fmt::Display for HookType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookType::Command => write!(f, "command"),
            HookType::Http => write!(f, "http"),
            HookType::Prompt => write!(f, "prompt"),
            HookType::Agent => write!(f, "agent"),
        }
    }
}

/// Hook definition in hooks.json registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    pub description: String,
    pub event: String,
    #[serde(rename = "type")]
    pub hook_type: HookType,
    /// For command hooks: script filename
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// For http hooks: endpoint URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// For http hooks: additional headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    /// For http hooks: allowed env vars for header interpolation
    #[serde(rename = "allowedEnvVars", skip_serializing_if = "Option::is_none")]
    pub allowed_env_vars: Option<Vec<String>>,
    /// For prompt/agent hooks: prompt text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// For prompt/agent hooks: model override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Matcher regex for tool/event filtering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    /// Status message shown during hook execution
    #[serde(rename = "statusMessage", skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    /// If true, runs asynchronously (command hooks only)
    #[serde(rename = "async", skip_serializing_if = "Option::is_none")]
    pub is_async: Option<bool>,
    /// Timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
}

pub type HookRegistry = BTreeMap<String, HookDef>;

pub fn execute(action: HookAction) -> Result<()> {
    match action {
        HookAction::Install { name, force } => install(name, force),
        HookAction::Uninstall { name } => uninstall(name),
        HookAction::List { json } => list(json),
        HookAction::Test { name, payload } => test_hook(&name, payload),
        HookAction::Serve { port } => serve(port),
        HookAction::Show { name } => show(&name),
    }
}

// ── List ──────────────────────────────────────────────────────────

fn list(json_output: bool) -> Result<()> {
    let registry = load_registry()?;
    let installed = load_installed_hooks()?;

    if json_output {
        let output: Vec<serde_json::Value> = registry
            .iter()
            .map(|(name, def)| {
                let is_installed = is_hook_installed(name, def, &installed);
                serde_json::json!({
                    "name": name,
                    "type": def.hook_type.to_string(),
                    "event": def.event,
                    "description": def.description,
                    "installed": is_installed,
                    "matcher": def.matcher,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    ui::section("Hooks");

    let mut table = ui::table::new_table();
    table.set_header(&["Status", "Type", "Name", "Event", "Description"]);
    for (name, def) in &registry {
        let is_installed = is_hook_installed(name, def, &installed);
        let icon = if is_installed {
            "✓".green().to_string()
        } else {
            "○".red().to_string()
        };
        let type_badge = match def.hook_type {
            HookType::Command => "cmd".blue().to_string(),
            HookType::Http => "http".magenta().to_string(),
            HookType::Prompt => "prompt".yellow().to_string(),
            HookType::Agent => "agent".cyan().to_string(),
        };
        let event_text = if let Some(ref m) = def.matcher {
            format!("{} (matcher: {})", def.event, m).dimmed().to_string()
        } else {
            def.event.dimmed().to_string()
        };
        ui::table::add_row(&mut table, &[
            icon.as_str(),
            type_badge.as_str(),
            name,
            event_text.as_str(),
            &def.description,
        ]);
    }
    println!("{table}");
    Ok(())
}

// ── Show ──────────────────────────────────────────────────────────

fn show(name: &str) -> Result<()> {
    let registry = load_registry()?;
    let def = registry
        .get(name)
        .with_context(|| format!("Hook '{}' not found in registry", name))?;

    ui::section(name);

    let mut table = ui::table::new_table();
    table.set_header(&["Property", "Value"]);
    ui::table::add_row(&mut table, &["Type", &def.hook_type.to_string()]);
    ui::table::add_row(&mut table, &["Event", &def.event]);
    ui::table::add_row(&mut table, &["Description", &def.description]);
    if let Some(ref matcher) = def.matcher {
        ui::table::add_row(&mut table, &["Matcher", matcher]);
    }
    if let Some(ref msg) = def.status_message {
        ui::table::add_row(&mut table, &["Status message", msg]);
    }
    if let Some(timeout) = def.timeout {
        ui::table::add_row(&mut table, &["Timeout", &format!("{}s", timeout)]);
    }

    match def.hook_type {
        HookType::Command => {
            if let Some(ref script) = def.script {
                ui::table::add_row(&mut table, &["Script", script]);
            }
            if def.is_async == Some(true) {
                ui::table::add_row(&mut table, &["Async", "true"]);
            }
        }
        HookType::Http => {
            if let Some(ref url) = def.url {
                ui::table::add_row(&mut table, &["URL", url]);
            }
            if let Some(ref headers) = def.headers {
                let header_text = headers
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect::<Vec<_>>()
                    .join("\n");
                ui::table::add_row(&mut table, &["Headers", &header_text]);
            }
            if let Some(ref vars) = def.allowed_env_vars {
                ui::table::add_row(&mut table, &["Allowed env vars", &vars.join(", ")]);
            }
        }
        HookType::Prompt | HookType::Agent => {
            if let Some(ref prompt) = def.prompt {
                let display = prompt_preview(prompt);
                ui::table::add_row(&mut table, &["Prompt", &display]);
            }
            if let Some(ref model) = def.model {
                ui::table::add_row(&mut table, &["Model", model]);
            }
        }
    }

    println!("{table}");
    Ok(())
}

// ── Install ───────────────────────────────────────────────────────

fn install(name: Option<String>, force: bool) -> Result<()> {
    let registry = load_registry()?;
    let hooks_source = hooks_source_dir()?;

    let to_install: Vec<(&String, &HookDef)> = match &name {
        Some(n) => {
            let def = registry
                .get(n.as_str())
                .with_context(|| format!("Hook '{}' not found in registry", n))?;
            vec![(
                registry.keys().find(|k| k.as_str() == n.as_str()).unwrap(),
                def,
            )]
        }
        None => registry.iter().collect(),
    };

    let hooks_target = config::global_hook_target();
    let settings_path = config::claude_settings_path();
    let registered = install_selected_hooks(
        &settings_path,
        &hooks_source,
        &hooks_target,
        &to_install,
        force,
    )?;

    ui::success(&format!(
        "{} hook(s) registered in settings.json",
        registered
    ));
    Ok(())
}

// ── Uninstall ─────────────────────────────────────────────────────

fn uninstall(name: Option<String>) -> Result<()> {
    let registry = load_registry()?;
    let hooks_target = config::global_hook_target();

    let to_remove: Vec<(&String, &HookDef)> = match &name {
        Some(n) => {
            let def = registry
                .get(n.as_str())
                .with_context(|| format!("Hook '{}' not found in registry", n))?;
            vec![(
                registry.keys().find(|k| k.as_str() == n.as_str()).unwrap(),
                def,
            )]
        }
        None => registry.iter().collect(),
    };

    let settings_path = config::claude_settings_path();
    uninstall_selected_hooks(&settings_path, &hooks_target, &to_remove)?;

    ui::success(&format!(
        "{} hook(s) removed from settings.json",
        to_remove.len()
    ));
    Ok(())
}

// ── Test ──────────────────────────────────────────────────────────

fn test_hook(name: &str, payload: Option<String>) -> Result<()> {
    let registry = load_registry()?;
    let def = registry
        .get(name)
        .with_context(|| format!("Hook '{}' not found", name))?;

    let test_payload = payload.unwrap_or_else(|| {
        serde_json::json!({
            "session_id": "test-session",
            "transcript_path": "/tmp/test-transcript.jsonl",
            "cwd": std::env::current_dir().unwrap_or_default().to_string_lossy(),
            "permission_mode": "default",
            "hook_event_name": def.event,
            "tool_name": "Bash",
            "tool_input": { "command": "echo hello" },
            "prompt": "test prompt from agt hook test"
        })
        .to_string()
    });

    ui::info(&format!(
        "Testing hook '{}' (type: {}, event: {})",
        name, def.hook_type, def.event
    ));

    match def.hook_type {
        HookType::Command => {
            let hooks_source = hooks_source_dir()?;
            let (script_path, output) =
                execute_command_hook(name, def, &hooks_source, &test_payload)?;

            ui::info(&format!("Running: bash {}", script_path.display()));

            eprintln!();
            eprintln!("  {} {}", "Exit code:".bold(), output.status);
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.is_empty() {
                eprintln!("  {}", "stdout:".bold());
                for line in stdout.lines() {
                    eprintln!("    {}", line);
                }
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                eprintln!("  {}", "stderr:".bold());
                for line in stderr.lines() {
                    eprintln!("    {}", line.yellow());
                }
            }
            ensure_command_success(name, &output.status)?;
        }
        HookType::Http => {
            let url = def.url.as_ref().with_context(|| "HTTP hook has no URL")?;
            ui::info(&format!("POST {}", url));
            let resp = ureq::post(url)
                .set("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(
                    def.timeout.unwrap_or(30) as u64
                ))
                .send_string(&test_payload)
                .with_context(|| format!("Hook '{}' HTTP request failed", name))?;
            let status = resp.status();
            ensure_http_success(name, status)?;
            let body = resp
                .into_string()
                .with_context(|| format!("Cannot read hook '{}' HTTP response", name))?;
            eprintln!();
            eprintln!("  {} {}", "Status:".bold(), status);
            if !body.is_empty() {
                eprintln!("  {}", "Response:".bold());
                // Try to pretty-print JSON
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    eprintln!(
                        "    {}",
                        serde_json::to_string_pretty(&json)
                            .unwrap_or(body)
                            .replace('\n', "\n    ")
                    );
                } else {
                    eprintln!("    {}", body);
                }
            }
        }
        HookType::Prompt | HookType::Agent => {
            let prompt_text = def
                .prompt
                .as_ref()
                .with_context(|| "Hook has no prompt defined")?;
            eprintln!();
            eprintln!("  {} {}", "Prompt:".bold(), prompt_text);
            eprintln!(
                "  {} {}",
                "Model:".bold(),
                def.model.as_deref().unwrap_or("(default)")
            );
            ui::info("Prompt/agent hooks are evaluated by Claude Code at runtime.");
            ui::info("The prompt will receive the event payload as $ARGUMENTS.");
        }
    }

    eprintln!();
    Ok(())
}

// ── Serve ─────────────────────────────────────────────────────────

fn serve(port: u16) -> Result<()> {
    let hooks_source = hooks_source_dir()?;
    let server_script = hooks_source.join("http/server.ts");

    if !server_script.exists() {
        bail!(
            "HTTP hook server not found at {}\nRun from the agt source directory or set AGT_DIR.",
            server_script.display()
        );
    }

    ui::info(&format!(
        "Starting HTTP hook server on port {}...",
        port
    ));
    ui::info(&format!("Server: {}", server_script.display()));
    eprintln!();

    // Try bun first, then deno, then npx tsx
    let runners: Vec<(&str, Vec<&str>)> = vec![
        ("bun", vec!["run"]),
        ("deno", vec!["run", "--allow-net", "--allow-read", "--allow-env"]),
        ("npx", vec!["tsx"]),
    ];

    for (runner, args) in &runners {
        if which_exists(runner) {
            let mut cmd_args: Vec<&str> = args.clone();
            cmd_args.push(server_script.to_str().unwrap());

            let status = std::process::Command::new(runner)
                .args(&cmd_args)
                .env("AGT_HOOK_PORT", port.to_string())
                .env(
                    "AGT_HOOKS_DIR",
                    hooks_source.to_str().unwrap_or_default(),
                )
                .status()
                .with_context(|| format!("Failed to start {} server", runner))?;

            if !status.success() {
                bail!("Hook server exited with status: {}", status);
            }
            return Ok(());
        }
    }

    bail!(
        "No TypeScript runtime found. Install one of: bun, deno, or tsx (npx tsx)\n  \
         npm install -g tsx"
    );
}

// ── Helpers ───────────────────────────────────────────────────────

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn hooks_source_dir() -> Result<PathBuf> {
    let source = config::find_source_dir()
        .or_else(config::find_cwd_source_dir)
        .with_context(|| config::source_dir_hint())?;
    Ok(source.join("hooks"))
}

fn load_registry() -> Result<HookRegistry> {
    let hooks_source = hooks_source_dir()?;
    let registry_path = hooks_source.join("hooks.json");

    if !registry_path.exists() {
        bail!("Hook registry not found: {}", registry_path.display());
    }

    let content = fs::read_to_string(&registry_path)
        .with_context(|| format!("Cannot read {}", registry_path.display()))?;
    let registry: HookRegistry = serde_json::from_str(&content)
        .with_context(|| format!("Invalid hooks.json: {}", registry_path.display()))?;
    validate_registry_payloads(&registry, &registry_path)?;

    Ok(registry)
}

fn validate_registry_payloads(registry: &HookRegistry, registry_path: &Path) -> Result<()> {
    for (name, def) in registry {
        let (kind, field, value) = match def.hook_type {
            HookType::Command => ("command", "script", def.script.as_deref()),
            HookType::Http => ("http", "url", def.url.as_deref()),
            HookType::Prompt => ("prompt", "prompt", def.prompt.as_deref()),
            HookType::Agent => ("agent", "prompt", def.prompt.as_deref()),
        };
        if value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            bail!(
                "Invalid hook '{}' in {}: {} hook requires a non-empty '{}' field",
                name,
                registry_path.display(),
                kind,
                field
            );
        }
    }
    Ok(())
}

#[derive(Debug)]
struct CommandScript {
    hook_name: String,
    script: String,
}

#[derive(Debug)]
struct CommandSource {
    command: CommandScript,
    canonical_source: PathBuf,
}

fn validate_command_script_names(hooks: &[(&String, &HookDef)]) -> Result<Vec<CommandScript>> {
    let mut scripts = BTreeMap::new();
    for (name, def) in hooks {
        if let HookType::Command = def.hook_type {
            let script = def
                .script
                .as_deref()
                .with_context(|| format!("Invalid command hook '{}': missing script", name))?;
            validate_command_script_name(name, script)?;
            scripts
                .entry(script.to_string())
                .or_insert_with(|| CommandScript {
                    hook_name: (*name).clone(),
                    script: script.to_string(),
                });
        }
    }
    Ok(scripts.into_values().collect())
}

fn validate_command_sources(
    hooks: &[(&String, &HookDef)],
    hooks_source: &Path,
) -> Result<Vec<CommandSource>> {
    validate_command_script_names(hooks)?
        .into_iter()
        .map(|command| {
            let canonical_source =
                validate_command_source(&command.hook_name, &command.script, hooks_source)?;
            Ok(CommandSource {
                command,
                canonical_source,
            })
        })
        .collect()
}

fn validate_command_script_name(name: &str, script: &str) -> Result<()> {
    let script_path = Path::new(script);
    let mut components = script_path.components();
    let is_single_normal = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !script.trim().is_empty();
    if !is_single_normal {
        bail!(
            "Invalid command hook '{}' script path '{}': expected one non-empty file name",
            name,
            script
        );
    }
    Ok(())
}

fn validate_command_source(name: &str, script: &str, hooks_source: &Path) -> Result<PathBuf> {
    let canonical_root = hooks_source.canonicalize().with_context(|| {
        format!(
            "Cannot resolve hooks source for command hook '{}': {}",
            name,
            hooks_source.display()
        )
    })?;
    let source = hooks_source.join(script);
    let canonical_source = source.canonicalize().with_context(|| {
        format!(
            "Cannot resolve command hook '{}' script source: {}",
            name,
            source.display()
        )
    })?;
    if !canonical_source.starts_with(&canonical_root) {
        bail!(
            "Invalid command hook '{}' script source '{}': resolved outside {}",
            name,
            canonical_source.display(),
            canonical_root.display()
        );
    }
    if !canonical_source.is_file() {
        bail!(
            "Invalid command hook '{}' script source '{}': expected a regular file",
            name,
            canonical_source.display()
        );
    }

    Ok(canonical_source)
}

fn canonical_hook_target(hooks_target: &Path) -> Result<Option<PathBuf>> {
    let metadata = match fs::symlink_metadata(hooks_target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Cannot inspect hook target {}", hooks_target.display()))
        }
    };
    if metadata.file_type().is_symlink() {
        bail!(
            "Unsafe hook target {}: the hook directory itself must not be a symlink",
            hooks_target.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "Invalid hook target {}: expected a directory",
            hooks_target.display()
        );
    }
    hooks_target
        .canonicalize()
        .map(Some)
        .with_context(|| format!("Cannot resolve hook target {}", hooks_target.display()))
}

fn validate_command_destination(
    command: &CommandScript,
    hooks_target: &Path,
    canonical_target: &Path,
) -> Result<(PathBuf, bool)> {
    let destination = hooks_target.join(&command.script);
    let parent = destination
        .parent()
        .with_context(|| format!("Hook destination has no parent: {}", destination.display()))?;
    let canonical_parent = parent.canonicalize().with_context(|| {
        format!(
            "Cannot resolve destination parent for command hook '{}': {}",
            command.hook_name,
            parent.display()
        )
    })?;
    if canonical_parent != canonical_target {
        bail!(
            "Unsafe command hook '{}' destination '{}': parent resolves outside {}",
            command.hook_name,
            destination.display(),
            canonical_target.display()
        );
    }

    let exists = match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                bail!(
                    "Invalid command hook '{}' destination '{}': expected a file or symlink",
                    command.hook_name,
                    destination.display()
                );
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Cannot inspect command hook '{}' destination: {}",
                    command.hook_name,
                    destination.display()
                )
            })
        }
    };
    Ok((destination, exists))
}

fn install_command_scripts(
    sources: &[CommandSource],
    hooks_target: &Path,
    force: bool,
) -> Result<()> {
    if sources.is_empty() {
        return Ok(());
    }

    let canonical_target = match canonical_hook_target(hooks_target)? {
        Some(target) => target,
        None => {
            fs::create_dir_all(hooks_target)
                .with_context(|| format!("Cannot create {}", hooks_target.display()))?;
            canonical_hook_target(hooks_target)?.with_context(|| {
                format!("Hook target was not created: {}", hooks_target.display())
            })?
        }
    };
    let destinations = sources
        .iter()
        .map(|source| {
            validate_command_destination(&source.command, hooks_target, &canonical_target)
        })
        .collect::<Result<Vec<_>>>()?;

    for (source, (destination, exists)) in sources.iter().zip(destinations) {
        if exists {
            if !force {
                ui::warn(&format!(
                    "Already exists (use -f to overwrite): {}",
                    source.command.script
                ));
                continue;
            }
            fs::remove_file(&destination)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source.canonical_source, &destination)?;
        ui::success(&format!(
            "Linked: {} ({})",
            source.command.hook_name, source.command.script
        ));
    }
    Ok(())
}

fn uninstall_command_scripts(scripts: &[CommandScript], hooks_target: &Path) -> Result<()> {
    if scripts.is_empty() {
        return Ok(());
    }
    let Some(canonical_target) = canonical_hook_target(hooks_target)? else {
        return Ok(());
    };
    let destinations = scripts
        .iter()
        .map(|script| validate_command_destination(script, hooks_target, &canonical_target))
        .collect::<Result<Vec<_>>>()?;

    for (script, (destination, exists)) in scripts.iter().zip(destinations) {
        if exists {
            fs::remove_file(&destination)?;
            ui::success(&format!("Removed script: {}", script.script));
        }
    }
    Ok(())
}

fn prompt_preview(prompt: &str) -> String {
    let mut chars = prompt.chars();
    let prefix: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{}...", prefix)
    } else {
        prefix
    }
}

fn ensure_command_success(name: &str, status: &std::process::ExitStatus) -> Result<()> {
    if !status.success() {
        bail!("Hook '{}' command failed with status {}", name, status);
    }
    Ok(())
}

fn execute_command_hook(
    name: &str,
    def: &HookDef,
    hooks_source: &Path,
    payload: &str,
) -> Result<(PathBuf, std::process::Output)> {
    let script = def
        .script
        .as_deref()
        .with_context(|| format!("Command hook '{}' has no script", name))?;
    validate_command_script_name(name, script)?;
    let script_path = validate_command_source(name, script, hooks_source)?;
    let output = std::process::Command::new("bash")
        .arg(&script_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(payload.as_bytes())?;
            }
            child.wait_with_output()
        })
        .with_context(|| {
            format!(
                "Failed to execute command hook '{}' from {}",
                name,
                script_path.display()
            )
        })?;
    Ok((script_path, output))
}

fn ensure_http_success(name: &str, status: u16) -> Result<()> {
    if !(200..300).contains(&status) {
        bail!("Hook '{}' HTTP request returned status {}", name, status);
    }
    Ok(())
}

fn load_installed_hooks() -> Result<serde_json::Value> {
    let settings_path = config::claude_settings_path();
    let Some(settings) = read_hook_settings(&settings_path)? else {
        return Ok(serde_json::json!({}));
    };
    Ok(settings
        .get("hooks")
        .cloned()
        .unwrap_or(serde_json::json!({})))
}

fn is_hook_installed(_name: &str, def: &HookDef, installed: &serde_json::Value) -> bool {
    let event = &def.event;
    if let Some(entries) = installed.get(event).and_then(|v| v.as_array()) {
        for entry in entries {
            if let Some(hooks) = entry.get("hooks").and_then(|v| v.as_array()) {
                for h in hooks {
                    match def.hook_type {
                        HookType::Command => {
                            if let Some(cmd) = h.get("command").and_then(|v| v.as_str()) {
                                if let Some(ref script) = def.script {
                                    if cmd.contains(script) {
                                        return true;
                                    }
                                }
                            }
                        }
                        HookType::Http => {
                            if let Some(url) = h.get("url").and_then(|v| v.as_str()) {
                                if let Some(ref def_url) = def.url {
                                    if url == def_url {
                                        return true;
                                    }
                                }
                            }
                        }
                        HookType::Prompt | HookType::Agent => {
                            if let Some(p) = h.get("prompt").and_then(|v| v.as_str()) {
                                if let Some(ref def_prompt) = def.prompt {
                                    // Compare first 80 chars
                                    let a: String = p.chars().take(80).collect();
                                    let b: String = def_prompt.chars().take(80).collect();
                                    if a == b {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

fn install_selected_hooks(
    settings_path: &Path,
    hooks_source: &Path,
    hooks_target: &Path,
    hooks: &[(&String, &HookDef)],
    force: bool,
) -> Result<usize> {
    let mut settings = read_hook_settings(settings_path)?.unwrap_or_else(|| serde_json::json!({}));

    // Resolve every command source and validate the complete settings shape
    // before creating the target directory, changing scripts, or registering
    // a handler.
    let command_sources = validate_command_sources(hooks, hooks_source)?;
    let registered = merge_hooks_into_settings(&mut settings, settings_path, hooks, hooks_target)?;

    install_command_scripts(&command_sources, hooks_target, force)?;
    write_hook_settings(settings_path, &settings)?;
    Ok(registered)
}

fn uninstall_selected_hooks(
    settings_path: &Path,
    hooks_target: &Path,
    hooks: &[(&String, &HookDef)],
) -> Result<()> {
    let mut settings = read_hook_settings(settings_path)?;
    if let Some(settings) = settings.as_mut() {
        remove_hooks_from_settings(settings, settings_path, hooks)?;
    }

    // Destination validation is independent of the original source, so stale
    // installed links remain removable after their source disappears.
    let command_scripts = validate_command_script_names(hooks)?;
    uninstall_command_scripts(&command_scripts, hooks_target)?;
    if let Some(settings) = settings {
        write_hook_settings(settings_path, &settings)?;
    }
    Ok(())
}

fn read_hook_settings(settings_path: &Path) -> Result<Option<serde_json::Value>> {
    if !settings_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(settings_path)
        .with_context(|| format!("Cannot read Claude settings: {}", settings_path.display()))?;
    let settings = serde_json::from_str(&content)
        .with_context(|| format!("Invalid Claude settings JSON: {}", settings_path.display()))?;
    validate_hook_settings_shape(&settings, settings_path)?;
    Ok(Some(settings))
}

fn validate_hook_settings_shape(settings: &serde_json::Value, settings_path: &Path) -> Result<()> {
    let root = settings.as_object().with_context(|| {
        format!(
            "Invalid Claude settings at {}: expected the root value to be an object",
            settings_path.display()
        )
    })?;
    let Some(hooks) = root.get("hooks") else {
        return Ok(());
    };
    let hooks = hooks.as_object().with_context(|| {
        format!(
            "Invalid Claude settings at {}: expected 'hooks' to be an object",
            settings_path.display()
        )
    })?;
    for (event, entries) in hooks {
        if !entries.is_array() {
            bail!(
                "Invalid Claude settings at {}: expected hook event '{}' to be an array",
                settings_path.display(),
                event
            );
        }
    }
    Ok(())
}

fn write_hook_settings(settings_path: &Path, settings: &serde_json::Value) -> Result<()> {
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(settings)?;
    fs::write(settings_path, content)
        .with_context(|| format!("Cannot write Claude settings: {}", settings_path.display()))?;
    Ok(())
}

fn merge_hooks_into_settings(
    settings: &mut serde_json::Value,
    settings_path: &Path,
    hooks: &[(&String, &HookDef)],
    hooks_target: &Path,
) -> Result<usize> {
    validate_hook_settings_shape(settings, settings_path)?;

    let hooks_obj = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let mut registered = 0;
    for (_name, def) in hooks {
        let event = &def.event;
        let event_arr = hooks_obj
            .as_object_mut()
            .unwrap()
            .entry(event.clone())
            .or_insert_with(|| serde_json::json!([]));

        let handler = build_handler_json(def, hooks_target);

        // Check for duplicates
        if !is_handler_duplicate(event_arr, &handler, def) {
            let mut entry = serde_json::json!({ "hooks": [handler] });
            if let Some(ref matcher) = def.matcher {
                entry
                    .as_object_mut()
                    .unwrap()
                    .insert("matcher".to_string(), serde_json::json!(matcher));
            }
            event_arr.as_array_mut().unwrap().push(entry);
            registered += 1;
        }
    }
    Ok(registered)
}

fn remove_hooks_from_settings(
    settings: &mut serde_json::Value,
    settings_path: &Path,
    hooks: &[(&String, &HookDef)],
) -> Result<()> {
    validate_hook_settings_shape(settings, settings_path)?;

    if let Some(hooks_obj) = settings.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for (_name, def) in hooks {
            let event = &def.event;
            if let Some(entries) = hooks_obj.get_mut(event).and_then(|v| v.as_array_mut()) {
                entries.retain(|entry| {
                    if let Some(hook_arr) = entry.get("hooks").and_then(|v| v.as_array()) {
                        for h in hook_arr {
                            match def.hook_type {
                                HookType::Command => {
                                    if let (Some(cmd), Some(ref script)) =
                                        (h.get("command").and_then(|v| v.as_str()), &def.script)
                                    {
                                        if cmd.contains(script) {
                                            return false;
                                        }
                                    }
                                }
                                HookType::Http => {
                                    if let (Some(url), Some(ref def_url)) =
                                        (h.get("url").and_then(|v| v.as_str()), &def.url)
                                    {
                                        if url == def_url.as_str() {
                                            return false;
                                        }
                                    }
                                }
                                HookType::Prompt | HookType::Agent => {
                                    if let (Some(p), Some(ref def_prompt)) =
                                        (h.get("prompt").and_then(|v| v.as_str()), &def.prompt)
                                    {
                                        let a: String = p.chars().take(80).collect();
                                        let b: String = def_prompt.chars().take(80).collect();
                                        if a == b {
                                            return false;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    true
                });

                // Clean up empty arrays
                if entries.is_empty() {
                    hooks_obj.remove(event);
                }
            }
        }

        // Clean up empty hooks object
        if hooks_obj.is_empty() {
            settings.as_object_mut().unwrap().remove("hooks");
        }
    }
    Ok(())
}

fn build_handler_json(def: &HookDef, hooks_target: &Path) -> serde_json::Value {
    let mut handler = serde_json::Map::new();

    match def.hook_type {
        HookType::Command => {
            handler.insert("type".into(), "command".into());
            if let Some(ref script) = def.script {
                let full_path = hooks_target.join(script);
                handler.insert(
                    "command".into(),
                    format!("bash {}", full_path.display()).into(),
                );
            }
            if def.is_async == Some(true) {
                handler.insert("async".into(), true.into());
            }
        }
        HookType::Http => {
            handler.insert("type".into(), "http".into());
            if let Some(ref url) = def.url {
                handler.insert("url".into(), url.clone().into());
            }
            if let Some(ref headers) = def.headers {
                handler.insert("headers".into(), serde_json::to_value(headers).unwrap());
            }
            if let Some(ref vars) = def.allowed_env_vars {
                handler.insert("allowedEnvVars".into(), serde_json::to_value(vars).unwrap());
            }
        }
        HookType::Prompt => {
            handler.insert("type".into(), "prompt".into());
            if let Some(ref prompt) = def.prompt {
                handler.insert("prompt".into(), prompt.clone().into());
            }
            if let Some(ref model) = def.model {
                handler.insert("model".into(), model.clone().into());
            }
        }
        HookType::Agent => {
            handler.insert("type".into(), "agent".into());
            if let Some(ref prompt) = def.prompt {
                handler.insert("prompt".into(), prompt.clone().into());
            }
            if let Some(ref model) = def.model {
                handler.insert("model".into(), model.clone().into());
            }
        }
    }

    if let Some(ref msg) = def.status_message {
        handler.insert("statusMessage".into(), msg.clone().into());
    }
    if let Some(timeout) = def.timeout {
        handler.insert("timeout".into(), timeout.into());
    }

    serde_json::Value::Object(handler)
}

fn is_handler_duplicate(
    event_arr: &serde_json::Value,
    _handler: &serde_json::Value,
    def: &HookDef,
) -> bool {
    if let Some(entries) = event_arr.as_array() {
        for entry in entries {
            if let Some(hooks) = entry.get("hooks").and_then(|v| v.as_array()) {
                for h in hooks {
                    match def.hook_type {
                        HookType::Command => {
                            if let (Some(cmd), Some(ref script)) =
                                (h.get("command").and_then(|v| v.as_str()), &def.script)
                            {
                                if cmd.contains(script) {
                                    return true;
                                }
                            }
                        }
                        HookType::Http => {
                            if let (Some(url), Some(ref def_url)) =
                                (h.get("url").and_then(|v| v.as_str()), &def.url)
                            {
                                if url == def_url.as_str() {
                                    return true;
                                }
                            }
                        }
                        HookType::Prompt | HookType::Agent => {
                            if let (Some(p), Some(ref def_prompt)) =
                                (h.get("prompt").and_then(|v| v.as_str()), &def.prompt)
                            {
                                let a: String = p.chars().take(80).collect();
                                let b: String = def_prompt.chars().take(80).collect();
                                if a == b {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_registry(value: serde_json::Value) -> HookRegistry {
        serde_json::from_value(value).expect("test registry should deserialize")
    }

    #[test]
    fn hook_payload_validation_accepts_each_valid_kind() {
        let cases = [
            serde_json::json!({
                "command-hook": {
                    "description": "command",
                    "event": "PreToolUse",
                    "type": "command",
                    "script": "check.sh"
                }
            }),
            serde_json::json!({
                "http-hook": {
                    "description": "http",
                    "event": "PreToolUse",
                    "type": "http",
                    "url": "https://example.test/hook"
                }
            }),
            serde_json::json!({
                "prompt-hook": {
                    "description": "prompt",
                    "event": "PreToolUse",
                    "type": "prompt",
                    "prompt": "Review the event"
                }
            }),
            serde_json::json!({
                "agent-hook": {
                    "description": "agent",
                    "event": "PreToolUse",
                    "type": "agent",
                    "prompt": "Inspect the event"
                }
            }),
        ];

        for case in cases {
            let registry = parse_registry(case);
            validate_registry_payloads(&registry, Path::new("/fixture/hooks.json")).unwrap();
        }
    }

    #[test]
    fn hook_payload_validation_rejects_missing_or_blank_fields_for_each_kind() {
        let cases = [
            (
                "command-hook",
                "script",
                serde_json::json!({
                    "command-hook": {
                        "description": "command",
                        "event": "PreToolUse",
                        "type": "command"
                    }
                }),
            ),
            (
                "http-hook",
                "url",
                serde_json::json!({
                    "http-hook": {
                        "description": "http",
                        "event": "PreToolUse",
                        "type": "http",
                        "url": "  "
                    }
                }),
            ),
            (
                "prompt-hook",
                "prompt",
                serde_json::json!({
                    "prompt-hook": {
                        "description": "prompt",
                        "event": "PreToolUse",
                        "type": "prompt"
                    }
                }),
            ),
            (
                "agent-hook",
                "prompt",
                serde_json::json!({
                    "agent-hook": {
                        "description": "agent",
                        "event": "PreToolUse",
                        "type": "agent",
                        "prompt": "\n\t"
                    }
                }),
            ),
        ];

        for (name, field, case) in cases {
            let registry = parse_registry(case);
            let error = validate_registry_payloads(&registry, Path::new("/fixture/hooks.json"))
                .unwrap_err()
                .to_string();
            assert!(error.contains(name), "{error}");
            assert!(error.contains(field), "{error}");
            assert!(error.contains("/fixture/hooks.json"), "{error}");
        }
    }

    #[test]
    fn command_script_validation_requires_one_contained_regular_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("hooks");
        fs::create_dir(&hooks_source).unwrap();
        let valid_source = hooks_source.join("check.sh");
        fs::write(&valid_source, "#!/bin/sh\n").unwrap();

        assert_eq!(
            validate_command_source("valid", "check.sh", &hooks_source).unwrap(),
            valid_source.canonicalize().unwrap()
        );
        for invalid in ["", ".", "..", "nested/check.sh", "/tmp/check.sh"] {
            assert!(validate_command_script_name("invalid", invalid).is_err());
        }

        fs::create_dir(hooks_source.join("directory")).unwrap();
        assert!(validate_command_source("directory", "directory", &hooks_source).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn command_script_validation_rejects_symlinks_outside_hooks_source() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("hooks");
        fs::create_dir(&hooks_source).unwrap();
        let sentinel = temp.path().join("outside.sh");
        fs::write(&sentinel, "outside sentinel").unwrap();
        std::os::unix::fs::symlink(&sentinel, hooks_source.join("escape.sh")).unwrap();

        let error = validate_command_source("escape", "escape.sh", &hooks_source)
            .unwrap_err()
            .to_string();
        assert!(error.contains("resolved outside"), "{error}");
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[test]
    fn command_script_validation_ignores_script_fields_on_other_hook_types() {
        let registry = parse_registry(serde_json::json!({
            "http-hook": {
                "description": "http",
                "event": "PreToolUse",
                "type": "http",
                "url": "https://example.test/hook",
                "script": "../outside.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();

        assert!(validate_command_script_names(&hooks).unwrap().is_empty());
    }

    #[test]
    fn malformed_settings_shapes_fail_before_hook_file_or_settings_mutation() {
        let cases = [
            (serde_json::json!([]), "root"),
            (serde_json::json!({ "hooks": [] }), "hooks"),
            (
                serde_json::json!({ "hooks": { "PreToolUse": {} } }),
                "PreToolUse",
            ),
        ];

        for (settings_value, expected_context) in cases {
            let temp = tempfile::TempDir::new().unwrap();
            let hooks_source = temp.path().join("source");
            let hooks_target = temp.path().join("target");
            let settings_path = temp.path().join("settings.json");
            fs::create_dir(&hooks_source).unwrap();
            fs::create_dir(&hooks_target).unwrap();
            fs::write(hooks_source.join("check.sh"), "#!/bin/sh\n").unwrap();
            let destination = hooks_target.join("check.sh");
            fs::write(&destination, "existing destination").unwrap();
            let original_settings = settings_value.to_string();
            fs::write(&settings_path, &original_settings).unwrap();
            let registry = parse_registry(serde_json::json!({
                "command-hook": {
                    "description": "command",
                    "event": "PreToolUse",
                    "type": "command",
                    "script": "check.sh"
                }
            }));
            let hooks: Vec<_> = registry.iter().collect();

            let install_error =
                install_selected_hooks(&settings_path, &hooks_source, &hooks_target, &hooks, true)
                    .unwrap_err();
            let install_message = format!("{install_error:#}");
            assert!(
                install_message.contains(expected_context),
                "{install_message}"
            );
            assert!(
                install_message.contains(&settings_path.display().to_string()),
                "{install_message}"
            );
            assert_eq!(
                fs::read_to_string(&settings_path).unwrap(),
                original_settings
            );
            assert_eq!(
                fs::read_to_string(&destination).unwrap(),
                "existing destination"
            );

            let uninstall_error =
                uninstall_selected_hooks(&settings_path, &hooks_target, &hooks).unwrap_err();
            let uninstall_message = format!("{uninstall_error:#}");
            assert!(
                uninstall_message.contains(expected_context),
                "{uninstall_message}"
            );
            assert_eq!(
                fs::read_to_string(&settings_path).unwrap(),
                original_settings
            );
            assert_eq!(
                fs::read_to_string(&destination).unwrap(),
                "existing destination"
            );
        }
    }

    #[test]
    fn missing_command_source_fails_before_destination_or_registration_mutation() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("source");
        let hooks_target = temp.path().join("target");
        let settings_path = temp.path().join("settings.json");
        fs::create_dir(&hooks_source).unwrap();
        fs::create_dir(&hooks_target).unwrap();
        let destination = hooks_target.join("missing.sh");
        fs::write(&destination, "existing destination").unwrap();
        let original_settings = r#"{"unrelated":{"keep":true}}"#;
        fs::write(&settings_path, original_settings).unwrap();
        let registry = parse_registry(serde_json::json!({
            "missing-command": {
                "description": "command",
                "event": "PreToolUse",
                "type": "command",
                "script": "missing.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();

        let error =
            install_selected_hooks(&settings_path, &hooks_source, &hooks_target, &hooks, true)
                .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("missing-command"), "{message}");
        assert!(message.contains("missing.sh"), "{message}");
        assert_eq!(
            fs::read_to_string(&settings_path).unwrap(),
            original_settings
        );
        assert_eq!(
            fs::read_to_string(destination).unwrap(),
            "existing destination"
        );
    }

    #[test]
    fn existing_command_destination_is_preserved_and_registration_count_is_exact() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("source");
        let hooks_target = temp.path().join("target");
        let settings_path = temp.path().join("settings.json");
        fs::create_dir(&hooks_source).unwrap();
        fs::create_dir(&hooks_target).unwrap();
        fs::write(hooks_source.join("check.sh"), "#!/bin/sh\n").unwrap();
        let destination = hooks_target.join("check.sh");
        fs::write(&destination, "existing destination").unwrap();
        fs::write(
            &settings_path,
            r#"{"unrelated":{"keep":true},"hooks":{"OtherEvent":[]}}"#,
        )
        .unwrap();
        let registry = parse_registry(serde_json::json!({
            "command-hook": {
                "description": "command",
                "event": "PreToolUse",
                "type": "command",
                "script": "check.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();

        assert_eq!(
            install_selected_hooks(&settings_path, &hooks_source, &hooks_target, &hooks, false,)
                .unwrap(),
            1
        );
        assert_eq!(
            install_selected_hooks(&settings_path, &hooks_source, &hooks_target, &hooks, false,)
                .unwrap(),
            0
        );
        assert_eq!(
            fs::read_to_string(destination).unwrap(),
            "existing destination"
        );
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(settings_path).unwrap()).unwrap();
        assert_eq!(settings["unrelated"], serde_json::json!({ "keep": true }));
        assert_eq!(settings["hooks"]["OtherEvent"], serde_json::json!([]));
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn hook_test_never_executes_traversal_absolute_or_outside_symlink_sources() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("hooks");
        fs::create_dir(&hooks_source).unwrap();
        let marker = temp.path().join("executed");
        let outside = temp.path().join("outside.sh");
        fs::write(
            &outside,
            format!("#!/bin/sh\nprintf executed > \"{}\"\n", marker.display()),
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, hooks_source.join("escape.sh")).unwrap();

        let scripts = [
            "../outside.sh".to_string(),
            outside.display().to_string(),
            "escape.sh".to_string(),
        ];
        for script in scripts {
            let registry = parse_registry(serde_json::json!({
                "command-hook": {
                    "description": "command",
                    "event": "PreToolUse",
                    "type": "command",
                    "script": script
                }
            }));
            let def = registry.get("command-hook").unwrap();

            assert!(execute_command_hook("command-hook", def, &hooks_source, "{}").is_err());
            assert!(!marker.exists(), "rejected script executed: {script}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn hook_test_executes_valid_contained_sources_and_preserves_exit_validation() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("hooks");
        fs::create_dir(&hooks_source).unwrap();
        let captured = temp.path().join("payload.json");
        let valid = hooks_source.join("valid.sh");
        fs::write(
            &valid,
            format!("#!/bin/sh\ncat > \"{}\"\n", captured.display()),
        )
        .unwrap();
        let failing = hooks_source.join("failing.sh");
        fs::write(&failing, "#!/bin/sh\nexit 7\n").unwrap();

        let valid_registry = parse_registry(serde_json::json!({
            "command-hook": {
                "description": "command",
                "event": "PreToolUse",
                "type": "command",
                "script": "valid.sh"
            }
        }));
        let (validated_path, output) = execute_command_hook(
            "command-hook",
            valid_registry.get("command-hook").unwrap(),
            &hooks_source,
            "{\"ok\":true}",
        )
        .unwrap();
        assert_eq!(validated_path, valid.canonicalize().unwrap());
        assert!(ensure_command_success("command-hook", &output.status).is_ok());
        assert_eq!(fs::read_to_string(captured).unwrap(), "{\"ok\":true}");

        let failing_registry = parse_registry(serde_json::json!({
            "command-hook": {
                "description": "command",
                "event": "PreToolUse",
                "type": "command",
                "script": "failing.sh"
            }
        }));
        let (_, output) = execute_command_hook(
            "command-hook",
            failing_registry.get("command-hook").unwrap(),
            &hooks_source,
            "{}",
        )
        .unwrap();
        assert!(ensure_command_success("command-hook", &output.status).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn install_deduplicates_command_hooks_that_share_a_script() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("source");
        let hooks_target = temp.path().join("target");
        fs::create_dir(&hooks_source).unwrap();
        let source = hooks_source.join("check.sh");
        fs::write(&source, "#!/bin/sh\n").unwrap();
        let registry = parse_registry(serde_json::json!({
            "command-a": {
                "description": "command a",
                "event": "PreToolUse",
                "type": "command",
                "script": "check.sh"
            },
            "command-b": {
                "description": "command b",
                "event": "PostToolUse",
                "type": "command",
                "script": "check.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();
        let sources = validate_command_sources(&hooks, &hooks_source).unwrap();

        assert_eq!(sources.len(), 1);
        install_command_scripts(&sources, &hooks_target, false).unwrap();
        assert_eq!(
            fs::read_link(hooks_target.join("check.sh")).unwrap(),
            source.canonicalize().unwrap()
        );
    }

    #[test]
    fn uninstall_deduplicates_command_hooks_that_share_a_script() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_target = temp.path().join("target");
        fs::create_dir(&hooks_target).unwrap();
        let destination = hooks_target.join("check.sh");
        fs::write(&destination, "installed hook").unwrap();
        let registry = parse_registry(serde_json::json!({
            "command-a": {
                "description": "command a",
                "event": "PreToolUse",
                "type": "command",
                "script": "check.sh"
            },
            "command-b": {
                "description": "command b",
                "event": "PostToolUse",
                "type": "command",
                "script": "check.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();
        let scripts = validate_command_script_names(&hooks).unwrap();

        assert_eq!(scripts.len(), 1);
        uninstall_command_scripts(&scripts, &hooks_target).unwrap();
        assert!(fs::symlink_metadata(destination).is_err());
    }

    #[test]
    fn command_batch_preflight_preserves_earlier_destination_when_later_is_invalid() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("source");
        let hooks_target = temp.path().join("target");
        fs::create_dir(&hooks_source).unwrap();
        fs::create_dir(&hooks_target).unwrap();
        fs::write(hooks_source.join("a.sh"), "new a").unwrap();
        fs::write(hooks_source.join("b.sh"), "new b").unwrap();
        let earlier = hooks_target.join("a.sh");
        fs::write(&earlier, "outside sentinel").unwrap();
        fs::create_dir(hooks_target.join("b.sh")).unwrap();
        let registry = parse_registry(serde_json::json!({
            "command-a": {
                "description": "command a",
                "event": "PreToolUse",
                "type": "command",
                "script": "a.sh"
            },
            "command-b": {
                "description": "command b",
                "event": "PostToolUse",
                "type": "command",
                "script": "b.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();
        let sources = validate_command_sources(&hooks, &hooks_source).unwrap();
        let scripts = validate_command_script_names(&hooks).unwrap();

        assert!(install_command_scripts(&sources, &hooks_target, true).is_err());
        assert_eq!(fs::read_to_string(&earlier).unwrap(), "outside sentinel");
        assert!(uninstall_command_scripts(&scripts, &hooks_target).is_err());
        assert_eq!(fs::read_to_string(earlier).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn install_rejects_symlinked_target_root_without_touching_outside_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_source = temp.path().join("source");
        let outside = temp.path().join("outside");
        fs::create_dir(&hooks_source).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(hooks_source.join("check.sh"), "#!/bin/sh\n").unwrap();
        let sentinel = outside.join("check.sh");
        fs::write(&sentinel, "outside sentinel").unwrap();
        let hooks_target = temp.path().join("target");
        std::os::unix::fs::symlink(&outside, &hooks_target).unwrap();

        let registry = parse_registry(serde_json::json!({
            "command-hook": {
                "description": "command",
                "event": "PreToolUse",
                "type": "command",
                "script": "check.sh"
            }
        }));
        let hooks: Vec<_> = registry.iter().collect();
        let sources = validate_command_sources(&hooks, &hooks_source).unwrap();

        let error = install_command_scripts(&sources, &hooks_target, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not be a symlink"), "{error}");
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_rejects_symlinked_target_root_without_touching_outside_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("check.sh");
        fs::write(&sentinel, "outside sentinel").unwrap();
        let hooks_target = temp.path().join("target");
        std::os::unix::fs::symlink(&outside, &hooks_target).unwrap();
        let scripts = vec![CommandScript {
            hook_name: "command-hook".to_string(),
            script: "check.sh".to_string(),
        }];

        let error = uninstall_command_scripts(&scripts, &hooks_target)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not be a symlink"), "{error}");
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_removes_stale_link_without_original_source() {
        let temp = tempfile::TempDir::new().unwrap();
        let hooks_target = temp.path().join("target");
        fs::create_dir(&hooks_target).unwrap();
        let destination = hooks_target.join("check.sh");
        std::os::unix::fs::symlink(temp.path().join("missing-source.sh"), &destination).unwrap();
        let scripts = vec![CommandScript {
            hook_name: "command-hook".to_string(),
            script: "check.sh".to_string(),
        }];

        uninstall_command_scripts(&scripts, &hooks_target).unwrap();

        assert!(fs::symlink_metadata(destination).is_err());
    }

    #[test]
    fn http_status_validation_accepts_only_success_responses() {
        assert!(ensure_http_success("http-hook", 200).is_ok());
        assert!(ensure_http_success("http-hook", 299).is_ok());
        assert!(ensure_http_success("http-hook", 302).is_err());
        assert!(ensure_http_success("http-hook", 500).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn command_status_validation_rejects_nonzero_exit() {
        use std::os::unix::process::ExitStatusExt;

        let success = std::process::ExitStatus::from_raw(0);
        let failure = std::process::ExitStatus::from_raw(7 << 8);
        assert!(ensure_command_success("command-hook", &success).is_ok());
        assert!(ensure_command_success("command-hook", &failure).is_err());
    }

    #[test]
    fn prompt_preview_preserves_ascii_limit_and_unicode_boundaries() {
        let ascii_80 = "a".repeat(80);
        assert_eq!(prompt_preview(&ascii_80), ascii_80);
        assert_eq!(
            prompt_preview(&"a".repeat(81)),
            format!("{}...", "a".repeat(80))
        );

        let korean = "가".repeat(81);
        assert_eq!(prompt_preview(&korean), format!("{}...", "가".repeat(80)));

        let emoji = format!("{}{}", "🙂".repeat(80), "🚀");
        assert_eq!(prompt_preview(&emoji), format!("{}...", "🙂".repeat(80)));
    }
}

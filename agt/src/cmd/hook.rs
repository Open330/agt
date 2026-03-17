use crate::{config, ui};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
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

    eprintln!();
    eprintln!(
        "{}  ({} installed / {} available)",
        "Hooks".cyan().bold(),
        "✓".green(),
        "○".red()
    );
    eprintln!("{}", "════════════════════════════════════════".cyan());
    eprintln!();

    for (name, def) in &registry {
        let is_installed = is_hook_installed(name, def, &installed);
        let icon = if is_installed {
            "✓".green()
        } else {
            "○".red()
        };
        let type_badge = match def.hook_type {
            HookType::Command => "cmd".blue(),
            HookType::Http => "http".magenta(),
            HookType::Prompt => "prompt".yellow(),
            HookType::Agent => "agent".cyan(),
        };
        let name_display = format!("{:<24}", name);
        let name_colored = if is_installed {
            name_display.green()
        } else {
            name_display.normal()
        };
        eprintln!(
            "  {} [{}] {} {}",
            icon, type_badge, name_colored, def.description
        );
        eprintln!(
            "       {} {}{}",
            "event:".dimmed(),
            def.event.dimmed(),
            def.matcher
                .as_ref()
                .map(|m| format!(" (matcher: {})", m))
                .unwrap_or_default()
                .dimmed()
        );
    }

    eprintln!();
    Ok(())
}

// ── Show ──────────────────────────────────────────────────────────

fn show(name: &str) -> Result<()> {
    let registry = load_registry()?;
    let def = registry
        .get(name)
        .with_context(|| format!("Hook '{}' not found in registry", name))?;

    eprintln!();
    eprintln!("  {} {}", "Hook:".bold(), name.green().bold());
    eprintln!("  {} {}", "Type:".bold(), def.hook_type.to_string().cyan());
    eprintln!("  {} {}", "Event:".bold(), def.event);
    eprintln!("  {} {}", "Description:".bold(), def.description);

    if let Some(ref matcher) = def.matcher {
        eprintln!("  {} {}", "Matcher:".bold(), matcher);
    }
    if let Some(ref msg) = def.status_message {
        eprintln!("  {} {}", "Status message:".bold(), msg);
    }
    if let Some(timeout) = def.timeout {
        eprintln!("  {} {}s", "Timeout:".bold(), timeout);
    }

    match def.hook_type {
        HookType::Command => {
            if let Some(ref script) = def.script {
                eprintln!("  {} {}", "Script:".bold(), script);
            }
            if def.is_async == Some(true) {
                eprintln!("  {} true", "Async:".bold());
            }
        }
        HookType::Http => {
            if let Some(ref url) = def.url {
                eprintln!("  {} {}", "URL:".bold(), url);
            }
            if let Some(ref headers) = def.headers {
                eprintln!("  {}", "Headers:".bold());
                for (k, v) in headers {
                    eprintln!("    {}: {}", k, v);
                }
            }
            if let Some(ref vars) = def.allowed_env_vars {
                eprintln!("  {} {}", "Allowed env vars:".bold(), vars.join(", "));
            }
        }
        HookType::Prompt | HookType::Agent => {
            if let Some(ref prompt) = def.prompt {
                let display = if prompt.len() > 80 {
                    format!("{}...", &prompt[..80])
                } else {
                    prompt.clone()
                };
                eprintln!("  {} {}", "Prompt:".bold(), display);
            }
            if let Some(ref model) = def.model {
                eprintln!("  {} {}", "Model:".bold(), model);
            }
        }
    }

    eprintln!();
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

    // Install command hook scripts to ~/.claude/hooks/
    let hooks_target = config::global_hook_target();
    fs::create_dir_all(&hooks_target)
        .with_context(|| format!("Cannot create {}", hooks_target.display()))?;

    for (hook_name, def) in &to_install {
        if let HookType::Command = def.hook_type {
            if let Some(ref script) = def.script {
                let src = hooks_source.join(script);
                let dst = hooks_target.join(script);
                if !src.exists() {
                    ui::warn(&format!("Script not found: {}", src.display()));
                    continue;
                }
                if dst.exists() {
                    if !force {
                        ui::warn(&format!("Already exists (use -f to overwrite): {}", script));
                        continue;
                    }
                    fs::remove_file(&dst)?;
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(&src, &dst)?;
                ui::success(&format!("Linked: {} ({})", hook_name, script));
            }
        }
    }

    // Merge hook config into settings.json
    let settings_path = config::claude_settings_path();
    merge_hooks_into_settings(&settings_path, &to_install, &hooks_target)?;

    ui::success(&format!(
        "{} hook(s) registered in settings.json",
        to_install.len()
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

    // Remove script symlinks
    for (_hook_name, def) in &to_remove {
        if let Some(ref script) = def.script {
            let dst = hooks_target.join(script);
            if dst.exists() {
                fs::remove_file(&dst)?;
                ui::success(&format!("Removed script: {}", script));
            }
        }
    }

    // Remove from settings.json
    let settings_path = config::claude_settings_path();
    remove_hooks_from_settings(&settings_path, &to_remove, &hooks_target)?;

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
            let script = def
                .script
                .as_ref()
                .with_context(|| "Command hook has no script")?;
            let script_path = hooks_source.join(script);

            if !script_path.exists() {
                bail!("Script not found: {}", script_path.display());
            }

            ui::info(&format!("Running: bash {}", script_path.display()));
            let output = std::process::Command::new("bash")
                .arg(&script_path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(ref mut stdin) = child.stdin {
                        let _ = stdin.write_all(test_payload.as_bytes());
                    }
                    child.wait_with_output()
                })?;

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
        }
        HookType::Http => {
            let url = def.url.as_ref().with_context(|| "HTTP hook has no URL")?;
            ui::info(&format!("POST {}", url));
            match ureq::post(url)
                .set("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(
                    def.timeout.unwrap_or(30) as u64,
                ))
                .send_string(&test_payload)
            {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.into_string().unwrap_or_default();
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
                Err(e) => {
                    ui::error(&format!("Request failed: {}", e));
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

    Ok(registry)
}

fn load_installed_hooks() -> Result<serde_json::Value> {
    let settings_path = config::claude_settings_path();
    if !settings_path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(&settings_path)?;
    let settings: serde_json::Value = serde_json::from_str(&content)?;
    Ok(settings.get("hooks").cloned().unwrap_or(serde_json::json!({})))
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

fn merge_hooks_into_settings(
    settings_path: &Path,
    hooks: &[(&String, &HookDef)],
    hooks_target: &Path,
) -> Result<()> {
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(settings_path)?;
        serde_json::from_str(&content)?
    } else {
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        serde_json::json!({})
    };

    let hooks_obj = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

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
        }
    }

    let content = serde_json::to_string_pretty(&settings)?;
    fs::write(settings_path, content)?;
    Ok(())
}

fn remove_hooks_from_settings(
    settings_path: &Path,
    hooks: &[(&String, &HookDef)],
    _hooks_target: &Path,
) -> Result<()> {
    if !settings_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(settings_path)?;
    let mut settings: serde_json::Value = serde_json::from_str(&content)?;

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

    let content = serde_json::to_string_pretty(&settings)?;
    fs::write(settings_path, content)?;
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

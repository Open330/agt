use crate::{config, ui};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum TeamAction {
    /// List available team templates
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show details of a team template
    Show {
        /// Team template name
        name: String,
    },
    /// Create a team from a template (generates prompt for Claude Code)
    Create {
        /// Team template name
        name: String,
        /// Override teammate count
        #[arg(short = 'n', long)]
        teammates: Option<usize>,
        /// Override teammate display mode (in-process, tmux, auto)
        #[arg(long)]
        mode: Option<String>,
        /// Additional context to pass to the team
        #[arg(long)]
        context: Option<String>,
    },
    /// Enable agent teams in Claude Code settings
    Enable,
    /// Disable agent teams in Claude Code settings
    Disable,
    /// Show current agent team status and settings
    Status,
    /// Initialize a project-local team template
    Init {
        /// Template name to base on (optional)
        #[arg(long)]
        from: Option<String>,
    },
}

// ── Team Template Definition ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTemplate {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub teammates: Vec<TeammateSpec>,
    #[serde(default)]
    pub tasks: Vec<TaskSpec>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub hooks: BTreeMap<String, Vec<HookSpec>>,
    #[serde(default = "default_teammate_mode")]
    pub teammate_mode: String,
    #[serde(default)]
    pub plan_approval: bool,
}

fn default_teammate_mode() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateSpec {
    pub role: String,
    pub description: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub plan_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

pub fn execute(action: TeamAction) -> Result<()> {
    match action {
        TeamAction::List { json } => list(json),
        TeamAction::Show { name } => show(&name),
        TeamAction::Create {
            name,
            teammates,
            mode,
            context,
        } => create(&name, teammates, mode, context),
        TeamAction::Enable => enable(),
        TeamAction::Disable => disable(),
        TeamAction::Status => status(),
        TeamAction::Init { from } => init(from),
    }
}

// ── List ──────────────────────────────────────────────────────────

fn list(json_output: bool) -> Result<()> {
    let templates = load_all_templates()?;

    if json_output {
        let output: Vec<serde_json::Value> = templates
            .iter()
            .map(|(name, t)| {
                serde_json::json!({
                    "name": name,
                    "description": t.description,
                    "teammates": t.teammates.len(),
                    "tasks": t.tasks.len(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    eprintln!();
    eprintln!("{}", "Agent Team Templates".cyan().bold());
    eprintln!("{}", "════════════════════════════════════════".cyan());
    eprintln!();

    if templates.is_empty() {
        eprintln!("  No team templates found.");
        eprintln!("  Create one with: agt team init");
        eprintln!();
        return Ok(());
    }

    for (name, template) in &templates {
        let name_display = format!("{:<20}", name);
        eprintln!(
            "  {} {} ({} teammates, {} tasks)",
            name_display.green().bold(),
            template.description,
            template.teammates.len().to_string().cyan(),
            template.tasks.len().to_string().yellow(),
        );
        for mate in &template.teammates {
            eprintln!(
                "    {} {}",
                format!("- {}", mate.role).dimmed(),
                mate.description.dimmed()
            );
        }
    }

    eprintln!();
    eprintln!("  Use: agt team create <name>  to generate a team spawn prompt");
    eprintln!("  Use: agt team show <name>    for full details");
    eprintln!();
    Ok(())
}

// ── Show ──────────────────────────────────────────────────────────

fn show(name: &str) -> Result<()> {
    let template = load_template(name)?;

    eprintln!();
    eprintln!("  {} {}", "Team:".bold(), template.name.green().bold());
    eprintln!("  {} {}", "Description:".bold(), template.description);
    eprintln!(
        "  {} {}",
        "Display mode:".bold(),
        template.teammate_mode.cyan()
    );
    eprintln!(
        "  {} {}",
        "Plan approval:".bold(),
        if template.plan_approval { "yes" } else { "no" }
    );

    if !template.skills.is_empty() {
        eprintln!("  {} {}", "Required skills:".bold(), template.skills.join(", "));
    }

    eprintln!();
    eprintln!("  {}", "Teammates:".bold());
    for (i, mate) in template.teammates.iter().enumerate() {
        eprintln!(
            "    {}. {} — {}",
            i + 1,
            mate.role.cyan().bold(),
            mate.description
        );
        if let Some(ref persona) = mate.persona {
            eprintln!("       {} {}", "persona:".dimmed(), persona);
        }
        if !mate.skills.is_empty() {
            eprintln!("       {} {}", "skills:".dimmed(), mate.skills.join(", "));
        }
        if mate.plan_approval {
            eprintln!("       {} required", "plan approval:".dimmed());
        }
    }

    if !template.tasks.is_empty() {
        eprintln!();
        eprintln!("  {}", "Initial tasks:".bold());
        for (i, task) in template.tasks.iter().enumerate() {
            let assignee = task
                .assignee
                .as_deref()
                .unwrap_or("unassigned");
            eprintln!(
                "    {}. {} [{}]",
                i + 1,
                task.title,
                assignee.yellow()
            );
            if !task.depends_on.is_empty() {
                eprintln!("       depends on: {}", task.depends_on.join(", "));
            }
        }
    }

    if !template.hooks.is_empty() {
        eprintln!();
        eprintln!("  {}", "Hooks:".bold());
        for (event, hooks) in &template.hooks {
            for h in hooks {
                eprintln!("    {} [{}]", event, h.hook_type.dimmed());
            }
        }
    }

    eprintln!();
    Ok(())
}

// ── Create ────────────────────────────────────────────────────────

fn create(
    name: &str,
    override_count: Option<usize>,
    override_mode: Option<String>,
    extra_context: Option<String>,
) -> Result<()> {
    let template = load_template(name)?;

    // Check if agent teams are enabled
    if !is_teams_enabled()? {
        ui::warn("Agent teams are not enabled in Claude Code settings.");
        ui::info("Run: agt team enable");
        eprintln!();
    }

    let mode = override_mode.as_deref().unwrap_or(&template.teammate_mode);

    // Build the prompt to paste into Claude Code
    let mut prompt = String::new();
    prompt.push_str(&format!(
        "Create an agent team for: {}\n\n",
        template.description
    ));

    prompt.push_str(&format!("Use {} display mode.\n\n", mode));

    if template.plan_approval {
        prompt.push_str("Require plan approval for all teammates before they make changes.\n\n");
    }

    prompt.push_str("## Teammates\n\n");

    let teammates = if let Some(count) = override_count {
        // If override, just take first N or repeat
        template.teammates.iter().take(count).collect::<Vec<_>>()
    } else {
        template.teammates.iter().collect::<Vec<_>>()
    };

    for mate in &teammates {
        prompt.push_str(&format!("### {} — {}\n", mate.role, mate.description));
        if let Some(ref persona) = mate.persona {
            prompt.push_str(&format!(
                "Load persona from: .agents/personas/{}.md\n",
                persona
            ));
        }
        if !mate.skills.is_empty() {
            prompt.push_str(&format!(
                "Required skills: {}\n",
                mate.skills.join(", ")
            ));
        }
        if mate.plan_approval {
            prompt.push_str("Require plan approval before making changes.\n");
        }
        prompt.push('\n');
    }

    if !template.tasks.is_empty() {
        prompt.push_str("## Initial Task List\n\n");
        for (i, task) in template.tasks.iter().enumerate() {
            prompt.push_str(&format!("{}. **{}**: {}\n", i + 1, task.title, task.description));
            if let Some(ref assignee) = task.assignee {
                prompt.push_str(&format!("   Assign to: {}\n", assignee));
            }
            if !task.depends_on.is_empty() {
                prompt.push_str(&format!(
                    "   Depends on: {}\n",
                    task.depends_on.join(", ")
                ));
            }
        }
        prompt.push('\n');
    }

    if let Some(ctx) = extra_context {
        prompt.push_str(&format!("## Additional Context\n\n{}\n", ctx));
    }

    // Output the prompt
    eprintln!();
    eprintln!(
        "{} Generated team prompt for '{}'",
        "[OK]".green(),
        template.name
    );
    eprintln!(
        "{} {} teammates, {} tasks, mode: {}",
        "[INFO]".blue(),
        teammates.len(),
        template.tasks.len(),
        mode
    );
    eprintln!();
    eprintln!(
        "{} Paste this into Claude Code to spawn the team:",
        "[INFO]".blue()
    );
    eprintln!("{}", "─".repeat(60).dimmed());

    // Print to stdout so it can be piped
    println!("{}", prompt);

    eprintln!("{}", "─".repeat(60).dimmed());
    eprintln!();
    eprintln!(
        "{} Or pipe directly: agt team create {} | pbcopy",
        "[TIP]".cyan(),
        name
    );
    eprintln!();

    Ok(())
}

// ── Enable/Disable ────────────────────────────────────────────────

fn enable() -> Result<()> {
    set_teams_setting(true)?;
    ui::success("Agent teams enabled in ~/.claude/settings.json");
    ui::info("Set env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = \"1\"");
    eprintln!();
    ui::info("Restart Claude Code for the change to take effect.");
    ui::info("Then tell Claude: \"Create an agent team to ...\"");
    Ok(())
}

fn disable() -> Result<()> {
    set_teams_setting(false)?;
    ui::success("Agent teams disabled in ~/.claude/settings.json");
    Ok(())
}

fn set_teams_setting(enabled: bool) -> Result<()> {
    let settings_path = config::claude_settings_path();

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content)?
    } else {
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        serde_json::json!({})
    };

    let obj = settings.as_object_mut().unwrap();

    if enabled {
        let env_obj = obj
            .entry("env")
            .or_insert_with(|| serde_json::json!({}));
        env_obj.as_object_mut().unwrap().insert(
            "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".to_string(),
            serde_json::json!("1"),
        );
    } else {
        if let Some(env_obj) = obj.get_mut("env").and_then(|v| v.as_object_mut()) {
            env_obj.remove("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS");
            if env_obj.is_empty() {
                obj.remove("env");
            }
        }
    }

    let content = serde_json::to_string_pretty(&settings)?;
    fs::write(&settings_path, content)?;
    Ok(())
}

// ── Status ────────────────────────────────────────────────────────

fn status() -> Result<()> {
    let enabled = is_teams_enabled()?;
    let mode = get_teammate_mode()?;
    let templates = load_all_templates()?;

    eprintln!();
    eprintln!("{}", "Agent Team Status".cyan().bold());
    eprintln!("{}", "════════════════════════════════════════".cyan());
    eprintln!();

    let enabled_str = if enabled {
        "enabled".green().bold()
    } else {
        "disabled".red().bold()
    };
    eprintln!("  {} {}", "Teams:".bold(), enabled_str);
    eprintln!(
        "  {} {}",
        "Teammate mode:".bold(),
        mode.unwrap_or_else(|| "auto (default)".to_string()).cyan()
    );
    eprintln!(
        "  {} {}",
        "Templates available:".bold(),
        templates.len()
    );

    // Check for active teams
    let teams_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/teams");
    if teams_dir.exists() {
        if let Ok(entries) = fs::read_dir(&teams_dir) {
            let active: Vec<String> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect();
            if !active.is_empty() {
                eprintln!();
                eprintln!("  {}", "Active teams:".bold());
                for team in &active {
                    eprintln!("    {} {}", "-".dimmed(), team.yellow());
                }
            }
        }
    }

    eprintln!();

    if !enabled {
        eprintln!(
            "  {} Run 'agt team enable' to activate agent teams",
            "->".yellow()
        );
        eprintln!();
    }

    Ok(())
}

// ── Init ──────────────────────────────────────────────────────────

fn init(from: Option<String>) -> Result<()> {
    let project_teams_dir = PathBuf::from(".claude/teams");
    fs::create_dir_all(&project_teams_dir)?;

    let template = if let Some(ref name) = from {
        load_template(name)?
    } else {
        TeamTemplate {
            name: "my-team".to_string(),
            description: "Custom agent team for this project".to_string(),
            teammates: vec![
                TeammateSpec {
                    role: "architect".to_string(),
                    description: "Designs the overall structure and reviews PRs".to_string(),
                    skills: vec![],
                    persona: None,
                    plan_approval: true,
                },
                TeammateSpec {
                    role: "implementer".to_string(),
                    description: "Writes code for assigned tasks".to_string(),
                    skills: vec![],
                    persona: None,
                    plan_approval: false,
                },
                TeammateSpec {
                    role: "tester".to_string(),
                    description: "Writes tests and verifies quality".to_string(),
                    skills: vec![],
                    persona: None,
                    plan_approval: false,
                },
            ],
            tasks: vec![],
            skills: vec![],
            hooks: BTreeMap::new(),
            teammate_mode: "auto".to_string(),
            plan_approval: false,
        }
    };

    let filename = format!("{}.yml", template.name);
    let target = project_teams_dir.join(&filename);

    if target.exists() {
        bail!(
            "Team template already exists: {}\nEdit it directly or remove first.",
            target.display()
        );
    }

    let yaml = serde_yaml::to_string(&template)?;
    fs::write(&target, yaml)?;

    ui::success(&format!("Created team template: {}", target.display()));
    ui::info("Edit the file to customize teammates, tasks, and hooks.");
    ui::info(&format!("Then run: agt team create {}", template.name));
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────

fn load_all_templates() -> Result<BTreeMap<String, TeamTemplate>> {
    let mut templates = BTreeMap::new();

    // 1. Bundled templates from source dir
    if let Some(source_dir) = config::find_source_dir() {
        load_templates_from_dir(&source_dir.join("teams"), &mut templates);
    }

    // 2. Global user templates
    let global_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/teams");
    load_templates_from_dir(&global_dir, &mut templates);

    // 3. Project-local templates (highest priority)
    let local_dir = PathBuf::from(".claude/teams");
    load_templates_from_dir(&local_dir, &mut templates);

    Ok(templates)
}

fn load_templates_from_dir(dir: &Path, templates: &mut BTreeMap<String, TeamTemplate>) {
    if !dir.exists() {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("yml") || ext == Some("yaml") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(template) = serde_yaml::from_str::<TeamTemplate>(&content) {
                        templates.insert(template.name.clone(), template);
                    }
                }
            }
        }
    }
}

fn load_template(name: &str) -> Result<TeamTemplate> {
    let templates = load_all_templates()?;
    templates
        .get(name)
        .cloned()
        .with_context(|| {
            let available: Vec<String> = templates.keys().cloned().collect();
            if available.is_empty() {
                format!(
                    "Team template '{}' not found. No templates available.\nCreate one with: agt team init",
                    name
                )
            } else {
                format!(
                    "Team template '{}' not found. Available: {}",
                    name,
                    available.join(", ")
                )
            }
        })
}

fn is_teams_enabled() -> Result<bool> {
    let settings_path = config::claude_settings_path();
    if !settings_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&settings_path)?;
    let settings: serde_json::Value = serde_json::from_str(&content)?;

    Ok(settings
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false))
}

fn get_teammate_mode() -> Result<Option<String>> {
    let settings_path = config::claude_settings_path();
    if !settings_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&settings_path)?;
    let settings: serde_json::Value = serde_json::from_str(&content)?;

    Ok(settings
        .get("teammateMode")
        .and_then(|v| v.as_str())
        .map(String::from))
}

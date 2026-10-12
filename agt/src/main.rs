mod cmd;
mod config;
mod doctor;
mod environment;
mod frontmatter;
mod gh;
mod llm;
mod remote;
mod ui;
mod util;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "agt",
    about = "agt — A modular toolkit for extending AI coding agents"
)]
#[command(version = VERSION)]
struct Cli {
    /// Claude Code config directory for skills, hooks, teams and settings.json
    /// (default: $CLAUDE_CONFIG_DIR, then ~/.claude)
    #[arg(long, global = true, value_name = "DIR")]
    claude_dir: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage agent skills
    Skill {
        #[command(subcommand)]
        action: cmd::skill::SkillAction,
    },
    /// Manage Claude Code hooks (command, http, prompt, agent)
    Hook {
        #[command(subcommand)]
        action: cmd::hook::HookAction,
    },
    /// Manage agent teams (spawn coordinated multi-agent workflows)
    #[command(
        long_about = "Manage agent teams — coordinated multi-agent workflows.\n\n\
            Agent teams let multiple Claude Code instances work together on parallel tasks.\n\
            Each teammate gets its own context window and can communicate with others.\n\n\
            Team templates define: teammates (roles), tasks (work items), hooks, and settings.\n\n\
            Template locations (searched in order):\n  \
              .claude/teams/           Project-local (highest priority)\n  \
              <claude-dir>/teams/      User global (~/.claude unless --claude-dir/CLAUDE_CONFIG_DIR)\n  \
              teams/                   Local source checkout\n\n\
            Quick start:\n  \
              agt team enable           Enable agent teams in Claude Code\n  \
              agt team list             See available team templates\n  \
              agt team create debug     Generate a spawn prompt for Claude Code\n  \
              agt team init             Create a custom team template"
    )]
    Team {
        #[command(subcommand)]
        action: cmd::team::TeamAction,
    },
    /// Manage agent personas (markdown files that define expert identities for any AI agent)
    #[command(
        long_about = "Manage agent personas — markdown files that define expert identities.\n\n\
            Personas are simple .md files with YAML frontmatter (name, role, domain, tags)\n\
            and a markdown body (identity, review lens, evaluation framework, output format).\n\
            Any AI agent can read and adopt a persona.\n\n\
            Persona locations (searched in order):\n  \
              .agents/personas/        Project-local (highest priority)\n  \
              ~/.agents/personas/      User global\n  \
              personas/                Local source checkout\n\n\
            Usage with different agents:\n  \
              Claude Code  Read the persona file path in conversation\n  \
              Codex        agt persona review <name> --codex\n  \
              Gemini       agt persona review <name> --gemini\n  \
              Any agent    cat .agents/personas/<name>.md | <agent-cli>"
    )]
    Persona {
        #[command(subcommand)]
        action: cmd::persona::PersonaAction,
    },
    /// Make skill directories match ~/.config/agt/layers.toml (install, adopt, prune)
    Apply {
        /// Only this target (`global` or a directory from layers.toml)
        #[arg(long, value_name = "PATH")]
        target: Option<String>,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
        /// Exit non-zero when anything would change (implies --dry-run)
        #[arg(long)]
        check: bool,
    },
    /// Run a command only when the Claude Code session is inside DIR (for hooks)
    Gate {
        /// Directory the session must be in ($CLAUDE_PROJECT_DIR, else cwd)
        dir: String,
        /// Command and arguments, after `--`
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Create agt.toml for this project (or the user environment with -g)
    Init {
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
        /// Agents to install skills for (default: claude)
        #[arg(long, value_enum, value_delimiter = ',')]
        agents: Vec<config::SkillAgent>,
    },
    /// Declare a dependency in agt.toml, lock it, and install it
    Add {
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long, global = true)]
        global: bool,
        #[command(subcommand)]
        kind: cmd::env::AddKind,
    },
    /// Remove a dependency from agt.toml and uninstall it
    Remove {
        /// Dependency name
        name: String,
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
    },
    /// Install exactly what agt.lock describes (re-locking manifest changes)
    Sync {
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
        /// Report differences without changing anything; exit 1 if any
        #[arg(long)]
        check: bool,
        /// Fail if agt.lock does not match agt.toml (for CI)
        #[arg(long)]
        frozen: bool,
    },
    /// Resolve agt.toml into agt.lock without installing
    Lock {
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
        /// Re-resolve these dependencies to their latest commit (all if none given)
        #[arg(long, num_args = 0.., value_name = "NAME", conflicts_with = "check")]
        update: Option<Vec<String>>,
        /// Exit 1 if agt.lock does not match agt.toml (offline; for CI)
        #[arg(long)]
        check: bool,
    },
    /// Show declared skills whose upstream changed since they were locked
    Outdated {
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Move skills to their latest commit after reviewing what changed
    Update {
        /// Skills to update (all if none given)
        names: Vec<String>,
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
        /// Do not prompt; apply only updates without executable changes
        #[arg(short, long)]
        yes: bool,
        /// Do not prompt; apply every update, including executable changes
        #[arg(long)]
        yes_all: bool,
    },
    /// Bring a skill installed by gh skill (or legacy agt) under agt.toml
    Adopt {
        /// Installed skill directory name
        name: String,
        /// Manage the user environment (~/.config/agt/agt.toml)
        #[arg(short, long)]
        global: bool,
    },
    /// Check installed skills: lock drift, duplicates, frontmatter, context budget
    Doctor {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Token budget for skill descriptions loaded per agent
        #[arg(long, default_value_t = 4000)]
        budget: usize,
        /// Do not ask `gh skill list` about other agent hosts
        #[arg(long)]
        no_gh: bool,
    },
    /// Run prompt with skill matching
    Run {
        /// The prompt to execute
        prompt: Vec<String>,
        /// Specify skill by name
        #[arg(long)]
        skill: Option<String>,
        /// LLM to use: claude, codex, opencode, gemini, ollama
        #[arg(long)]
        llm: Option<String>,
    },
    /// Generate shell completion scripts
    Completions {
        /// Shell type
        shell: Shell,
    },
    /// List names for shell completion (internal)
    #[command(hide = true)]
    CompleteNames {
        /// Type: "persona" or "skill"
        kind: String,
    },
    /// Show version
    Version,
}

fn main() {
    let cli = Cli::parse();
    if let Some(dir) = cli.claude_dir.as_deref() {
        config::set_claude_dir_override(dir);
    }

    let result = match cli.command {
        Commands::Skill { action } => cmd::skill::execute(action),
        Commands::Hook { action } => cmd::hook::execute(action),
        Commands::Team { action } => cmd::team::execute(action),
        Commands::Persona { action } => cmd::persona::execute(action),
        Commands::Apply {
            target,
            dry_run,
            check,
        } => cmd::apply::execute(target.as_deref(), dry_run, check),
        Commands::Gate { dir, command } => cmd::gate::execute(&dir, &command),
        Commands::Init { global, agents } => cmd::env::init(global, agents),
        Commands::Add { global, kind } => cmd::env::add(global, kind),
        Commands::Remove { name, global } => cmd::env::remove(global, &name),
        Commands::Sync {
            global,
            check,
            frozen,
        } => cmd::env::sync(global, check, frozen),
        Commands::Lock {
            global,
            update,
            check,
        } => cmd::env::lock(global, update, check),
        Commands::Outdated { global, json } => cmd::env::outdated(global, json),
        Commands::Update {
            names,
            global,
            yes,
            yes_all,
        } => cmd::env::update(global, names, yes, yes_all),
        Commands::Adopt { name, global } => cmd::env::adopt(global, &name),
        Commands::Doctor {
            json,
            budget,
            no_gh,
        } => doctor::execute(json, budget, no_gh),
        Commands::Run { prompt, skill, llm } => {
            cmd::run::execute(&prompt.join(" "), skill.as_deref(), llm.as_deref())
        }
        Commands::Completions { shell } => {
            generate_completions(shell);
            Ok(())
        }
        Commands::CompleteNames { kind } => {
            complete_names(&kind);
            Ok(())
        }
        Commands::Version => {
            println!("agt {}", VERSION);
            Ok(())
        }
    };

    if let Err(e) = result {
        ui::error(&format!("{:#}", e));
        std::process::exit(1);
    }
}

fn generate_completions(shell: Shell) {
    let mut cmd = Cli::command();

    // Print the base completion script
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "agt", &mut buf);
    let script = String::from_utf8(buf).unwrap_or_default();

    match shell {
        Shell::Zsh => print_zsh_completions(&script),
        Shell::Bash => print_bash_completions(&script),
        Shell::Fish => print_fish_completions(&script),
        _ => print!("{}", script),
    }
}

fn print_zsh_completions(base: &str) {
    // Print base completions from clap
    print!("{}", base);

    // Add dynamic completion functions
    println!(
        r#"
# Dynamic completions for persona and skill names
_agt_persona_names() {{
    local -a names
    names=(${{(f)"$(agt complete-names persona 2>/dev/null)"}})
    compadd -a names
}}

_agt_skill_names() {{
    local -a names
    names=(${{(f)"$(agt complete-names skill 2>/dev/null)"}})
    compadd -a names
}}

# Override persona subcommand completions
_agt_persona_review() {{
    _arguments \
        '1:persona name:_agt_persona_names' \
        '--codex[Use Codex]' \
        '--claude[Use Claude]' \
        '--gemini[Use Gemini]' \
        '--staged[Staged changes only]' \
        '--base=[Base branch]:branch:' \
        '-o=[Output file]:file:_files' \
        '*:prompt:'
}}

_agt_persona_install() {{
    _arguments \
        '1:persona name:_agt_persona_names' \
        '-g[Install globally]' \
        '--global[Install globally]' \
        '--agent=[Target agent]:agent:(claude codex)' \
        '-f[Force overwrite]' \
        '--force[Force overwrite]' \
        '-a[Install all]' \
        '--all[Install all]' \
        '--from=[Remote spec]:spec:'
}}

_agt_persona_uninstall() {{
    _arguments \
        '1:persona name:_agt_persona_names' \
        '-g[Global scope]' \
        '--global[Global scope]' \
        '-a[Uninstall all]' \
        '--all[Uninstall all]'
}}

_agt_persona_show() {{
    _arguments '1:persona name:_agt_persona_names'
}}

_agt_persona_which() {{
    _arguments '1:persona name:_agt_persona_names'
}}

_agt_team_names() {{
    local -a names
    names=(${{(f)"$(agt complete-names team 2>/dev/null)"}})
    compadd -a names
}}

_agt_team_create() {{
    _arguments \
        '1:team name:_agt_team_names' \
        '-n=[Teammate count]:count:' \
        '--teammates=[Teammate count]:count:' \
        '--mode=[Display mode]:mode:(in-process tmux auto)' \
        '--context=[Additional context]:context:'
}}

_agt_team_show() {{
    _arguments '1:team name:_agt_team_names'
}}

_agt_hook_names() {{
    local -a names
    names=(${{(f)"$(agt complete-names hook 2>/dev/null)"}})
    compadd -a names
}}

_agt_hook_install() {{
    _arguments \
        '1:hook name:_agt_hook_names' \
        '-f[Force overwrite]' \
        '--force[Force overwrite]'
}}

_agt_hook_uninstall() {{
    _arguments '1:hook name:_agt_hook_names'
}}

_agt_hook_test() {{
    _arguments \
        '1:hook name:_agt_hook_names' \
        '--payload=[JSON payload]:payload:'
}}

_agt_hook_show() {{
    _arguments '1:hook name:_agt_hook_names'
}}

_agt_skill_install() {{
    _arguments \
        '1:skill name:_agt_skill_names' \
        '-g[Install globally]' \
        '--global[Install globally]' \
        '-f[Force overwrite]' \
        '--force[Force overwrite]' \
        '-p[Install profile]:profile:(core dev agents integrations ml full all)' \
        '--profile=[Install profile]:profile:(core dev agents integrations ml full all)' \
        '-a[Install all skills]' \
        '--all[Install all skills]' \
        '--from=[Remote spec]:spec:'
}}

_agt_skill_uninstall() {{
    _arguments \
        '1:skill name:_agt_skill_names' \
        '-g[Global scope]' \
        '--global[Global scope]' \
        '--agent=[Target agent]:agent:(claude codex)'
}}

_agt_skill_which() {{
    _arguments \
        '1:skill name:_agt_skill_names' \
        '--agent=[Target agent]:agent:(claude codex)'
}}

_agt_skill_update() {{
    _arguments \
        '1:skill name:_agt_skill_names' \
        '-g[Global only]' \
        '--global[Global only]' \
        '-l[Local only]' \
        '--local[Local only]' \
        '--agent=[Target agent]:agent:(claude codex)'
}}
"#
    );
}

fn print_bash_completions(base: &str) {
    print!("{}", base);

    println!(
        r#"
# Dynamic completions for persona and skill names
_agt_dynamic_complete() {{
    local kind="$1"
    COMPREPLY=($(compgen -W "$(agt complete-names "$kind" 2>/dev/null)" -- "${{COMP_WORDS[COMP_CWORD]}}"))
}}

# Extend the generated completion
_agt_completion_orig=$(_agt_completion 2>/dev/null || true)

_agt_enhanced() {{
    local cur prev words cword
    _init_completion || return

    # Detect context: agt persona <subcommand> <NAME>
    if [[ "${{words[1]}}" == "persona" ]] && [[ $cword -ge 3 ]]; then
        case "${{words[2]}}" in
            review|install|uninstall|show|which)
                if [[ $cword -eq 3 ]] && [[ "$cur" != -* ]]; then
                    _agt_dynamic_complete persona
                    return
                fi
                ;;
        esac
    fi

    # Detect context: agt team <subcommand> <NAME>
    if [[ "${{words[1]}}" == "team" ]] && [[ $cword -ge 3 ]]; then
        case "${{words[2]}}" in
            create|show)
                if [[ $cword -eq 3 ]] && [[ "$cur" != -* ]]; then
                    _agt_dynamic_complete team
                    return
                fi
                ;;
        esac
    fi

    # Detect context: agt hook <subcommand> <NAME>
    if [[ "${{words[1]}}" == "hook" ]] && [[ $cword -ge 3 ]]; then
        case "${{words[2]}}" in
            install|uninstall|test|show)
                if [[ $cword -eq 3 ]] && [[ "$cur" != -* ]]; then
                    _agt_dynamic_complete hook
                    return
                fi
                ;;
        esac
    fi

    # Detect context: agt skill <subcommand> <NAME>
    if [[ "${{words[1]}}" == "skill" ]] && [[ $cword -ge 3 ]]; then
        case "${{words[2]}}" in
            install|uninstall|which|update)
                if [[ $cword -eq 3 ]] && [[ "$cur" != -* ]]; then
                    _agt_dynamic_complete skill
                    return
                fi
                ;;
        esac
    fi

    # Fall back to generated completions
    _agt "$@"
}}
complete -F _agt_enhanced -o nosort -o bashdefault -o default agt
"#
    );
}

fn print_fish_completions(base: &str) {
    print!("{}", base);

    println!(
        r#"
# Dynamic completions for persona names
complete -c agt -n '__fish_seen_subcommand_from persona; and __fish_seen_subcommand_from review install uninstall show which' -xa '(agt complete-names persona 2>/dev/null)'

# Dynamic completions for team names
complete -c agt -n '__fish_seen_subcommand_from team; and __fish_seen_subcommand_from create show' -xa '(agt complete-names team 2>/dev/null)'

# Dynamic completions for hook names
complete -c agt -n '__fish_seen_subcommand_from hook; and __fish_seen_subcommand_from install uninstall test show' -xa '(agt complete-names hook 2>/dev/null)'

# Dynamic completions for skill names
complete -c agt -n '__fish_seen_subcommand_from skill; and __fish_seen_subcommand_from install uninstall which update' -xa '(agt complete-names skill 2>/dev/null)'
"#
    );
}

/// Output names for shell completion
fn complete_names(kind: &str) {
    match kind {
        "persona" => {
            // Collect from all sources: local, global, library
            let mut names = std::collections::BTreeSet::new();

            // Local
            collect_names_from_dir(&config::local_persona_target(), &mut names);
            // Global
            collect_names_from_dir(&config::global_persona_target(), &mut names);
            // Library
            if let Some(source_dir) = config::find_source_dir() {
                collect_names_from_dir(&config::persona_library(&source_dir), &mut names);
            }

            for name in names {
                println!("{}", name);
            }
        }
        "team" => {
            let mut names = std::collections::BTreeSet::new();
            // Bundled templates
            if let Some(source_dir) = config::find_source_dir() {
                collect_yaml_names(&source_dir.join("teams"), &mut names);
            }
            // Global templates
            let global_dir = config::global_team_target();
            collect_yaml_names(&global_dir, &mut names);
            // Local templates
            collect_yaml_names(&std::path::PathBuf::from(".claude/teams"), &mut names);
            for name in names {
                println!("{}", name);
            }
        }
        "hook" => {
            let mut names = std::collections::BTreeSet::new();
            if let Some(source_dir) = config::find_source_dir() {
                let registry_path = source_dir.join("hooks/hooks.json");
                if let Ok(content) = std::fs::read_to_string(&registry_path) {
                    if let Ok(registry) = serde_json::from_str::<
                        std::collections::BTreeMap<String, serde_json::Value>,
                    >(&content)
                    {
                        for name in registry.keys() {
                            names.insert(name.clone());
                        }
                    }
                }
            }
            for name in names {
                println!("{}", name);
            }
        }
        "skill" => {
            let mut names = std::collections::BTreeSet::new();

            // Local
            collect_names_from_dir(&config::local_skill_target(), &mut names);
            collect_names_from_dir(&config::local_codex_skill_target(), &mut names);
            // Global
            collect_names_from_dir(&config::global_skill_target(), &mut names);
            collect_names_from_dir(&config::global_codex_skill_target(), &mut names);
            // Library
            if let Some(source_dir) = config::find_source_dir() {
                for group in config::skill_groups(&source_dir) {
                    for skill in config::skills_in_group(&source_dir, &group) {
                        names.insert(skill);
                    }
                }
            }

            for name in names {
                println!("{}", name);
            }
        }
        _ => {}
    }
}

fn collect_yaml_names(dir: &std::path::Path, names: &mut std::collections::BTreeSet<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let raw = entry.file_name().to_string_lossy().to_string();
            if let Some(stem) = raw
                .strip_suffix(".yml")
                .or_else(|| raw.strip_suffix(".yaml"))
            {
                names.insert(stem.to_string());
            }
        }
    }
}

fn collect_names_from_dir(dir: &std::path::Path, names: &mut std::collections::BTreeSet<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let raw = entry.file_name().to_string_lossy().to_string();
            if raw.starts_with('.') || raw == "README.md" {
                continue;
            }
            let name = raw.strip_suffix(".md").unwrap_or(&raw).to_string();
            names.insert(name);
        }
    }
}

use crate::{config, frontmatter, llm, ui, util};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Execute a prompt, optionally using a specific skill and/or LLM
pub fn execute(prompt: &str, skill: Option<&str>, llm_name: Option<&str>) -> Result<()> {
    if prompt.trim().is_empty() {
        bail!("No prompt provided.");
    }

    // Load explicit skill or auto-match from installed skills
    let skill_content = if let Some(skill_name) = skill {
        Some(load_skill(skill_name)?)
    } else {
        auto_match_skills(prompt)
    };

    // Use specified LLM or auto-detect (prefer claude for non-interactive)
    let cli = if let Some(name) = llm_name {
        llm::parse_cli(name)?
    } else {
        llm::detect_prefer_claude().context(
            "No LLM CLI found. Install claude, codex, opencode, gemini, or ollama.",
        )?
    };

    // Build final prompt
    let full_prompt = if let Some(ref skill_text) = skill_content {
        format!(
            "{}\n\n---\n\nUser request:\n{}",
            skill_text, prompt
        )
    } else {
        prompt.to_string()
    };

    ui::info(&format!("Running with {}...", cli));

    llm::invoke(cli, &full_prompt)?;

    Ok(())
}

fn load_skill(name: &str) -> Result<String> {
    util::validate_name(name)?;
    let local = config::local_skill_target().join(name);
    let global = config::global_skill_target().join(name);

    let skill_dir = if local.exists() {
        local
    } else if global.exists() {
        global
    } else if let Some(source_dir) = config::find_source_dir().or_else(config::find_cwd_source_dir) {
        find_skill_in_source(&source_dir, name)
            .context(format!("Skill '{}' not found", name))?
    } else {
        bail!("Skill '{}' not found", name);
    };

    let skill_md = skill_dir.join("SKILL.md");
    fs::read_to_string(&skill_md)
        .context(format!("Failed to read {}", skill_md.display()))
}

/// Auto-match multiple skills from installed skills based on prompt content.
fn auto_match_skills(prompt: &str) -> Option<String> {
    let prompt_lower = prompt.to_lowercase();
    let mut scored: Vec<(u32, String, String)> = Vec::new(); // (score, name, content)
    let mut seen = HashSet::new();

    // Scan installed skills (local then global, with subgroups)
    for dir in &[config::local_skill_target(), config::global_skill_target()] {
        collect_scored_skills(dir, &prompt_lower, &mut scored, &mut seen);
    }

    // Scan library skills
    if let Some(source_dir) = config::find_source_dir().or_else(config::find_cwd_source_dir) {
        for group in config::skill_groups(&source_dir) {
            collect_scored_skills(&source_dir.join(&group), &prompt_lower, &mut scored, &mut seen);
        }
    }

    if scored.is_empty() {
        return None;
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0));

    let names: Vec<&str> = scored.iter().map(|(_, n, _)| n.as_str()).collect();
    ui::info(&format!("Matched skills: {}", names.join(", ")));

    let combined: Vec<String> = scored.into_iter().map(|(_, _, content)| content).collect();
    Some(combined.join("\n\n---\n\n"))
}

fn collect_scored_skills(
    dir: &Path,
    prompt_lower: &str,
    scored: &mut Vec<(u32, String, String)>,
    seen: &mut HashSet<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if skill_md.exists() {
            score_skill(&path, &entry.file_name().to_string_lossy(), prompt_lower, scored, seen);
        } else {
            // Group directory — scan subdirectories
            let Ok(sub_entries) = fs::read_dir(&path) else { continue };
            for sub_entry in sub_entries.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.is_dir() && sub_path.join("SKILL.md").exists() {
                    let name = format!(
                        "{}/{}",
                        entry.file_name().to_string_lossy(),
                        sub_entry.file_name().to_string_lossy()
                    );
                    score_skill(&sub_path, &name, prompt_lower, scored, seen);
                }
            }
        }
    }
}

fn score_skill(
    path: &Path,
    name: &str,
    prompt_lower: &str,
    scored: &mut Vec<(u32, String, String)>,
    seen: &mut HashSet<String>,
) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let key = canonical.to_string_lossy().to_string();
    if seen.contains(&key) {
        return;
    }

    let skill_md = path.join("SKILL.md");
    let content = match fs::read_to_string(&skill_md) {
        Ok(c) => c,
        Err(_) => return,
    };
    let (fm, _) = match frontmatter::parse(&content) {
        Ok(parsed) => parsed,
        Err(_) => return,
    };

    let mut score = 0u32;

    if let Some(keywords) = &fm.trigger_keywords {
        for kw in keywords {
            if prompt_lower.contains(&kw.to_lowercase()) {
                score += 10;
            }
        }
    }

    for segment in name.split('/') {
        if prompt_lower.contains(&segment.to_lowercase()) {
            score += 5;
        }
    }

    if let Some(tags) = &fm.tags {
        for tag in tags {
            if prompt_lower.contains(&tag.to_lowercase()) {
                score += 2;
            }
        }
    }

    if score > 0 {
        seen.insert(key);
        scored.push((score, name.to_string(), content));
    }
}

fn find_skill_in_source(source_dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    for group in config::skill_groups(source_dir) {
        let path = source_dir.join(&group).join(name);
        if path.is_dir() && path.join("SKILL.md").exists() {
            return Some(path);
        }
    }
    None
}

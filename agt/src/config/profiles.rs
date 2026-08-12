use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileDef {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[allow(dead_code)]
pub struct ResolvedProfile {
    pub name: String,
    pub description: String,
    pub skills: Vec<(String, String)>, // (group, skill_name)
}

fn builtin_profiles() -> BTreeMap<String, ProfileDef> {
    let mut map = BTreeMap::new();
    map.insert(
        "core".to_string(),
        ProfileDef {
            description: "Essential skills for every workspace".to_string(),
            skills: vec![
                "development/git-commit-pr".into(),
                "context/context-manager".into(),
                "context/static-index".into(),
                "security/security-auditor".into(),
                "agents/background-implementer".into(),
                "agents/background-planner".into(),
                "agents/background-reviewer".into(),
            ],
            groups: vec![],
        },
    );
    map
}

fn read_profiles(path: &Path) -> Result<Option<BTreeMap<String, ProfileDef>>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read profiles file {}", path.display()))
        }
    };
    let profiles = serde_yaml::from_str::<BTreeMap<String, ProfileDef>>(&content)
        .with_context(|| format!("Invalid profiles YAML: {}", path.display()))?;
    Ok(Some(profiles))
}

fn load_profiles_file(source_dir: &Path) -> Result<Option<BTreeMap<String, ProfileDef>>> {
    let mut merged = BTreeMap::new();

    // Try profiles.yml first (canonical name)
    let canonical = source_dir.join("profiles.yml");
    if let Some(profiles) = read_profiles(&canonical)? {
        merged.extend(profiles);
    }

    // Also scan all *.yml files at root (repos may split profiles across files)
    let entries = match std::fs::read_dir(source_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((!merged.is_empty()).then_some(merged))
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to read profiles directory {}", source_dir.display())
            })
        }
    };
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "Failed to read an entry in profiles directory {}",
                source_dir.display()
            )
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yml")
            && path.file_name().unwrap_or_default() != "profiles.yml"
        {
            if let Some(profiles) = read_profiles(&path)? {
                merged.extend(profiles);
            }
        }
    }

    Ok((!merged.is_empty()).then_some(merged))
}

fn available_profiles(source_dir: &Path) -> Result<BTreeMap<String, ProfileDef>> {
    available_profiles_with_builtins(source_dir, true)
}

fn available_profiles_with_builtins(
    source_dir: &Path,
    include_builtins: bool,
) -> Result<BTreeMap<String, ProfileDef>> {
    let mut profiles = if include_builtins {
        builtin_profiles()
    } else {
        BTreeMap::new()
    };
    if let Some(file_profiles) = load_profiles_file(source_dir)? {
        for (name, def) in file_profiles {
            profiles.insert(name, def);
        }
    }
    Ok(profiles)
}

pub fn resolve_profile(name: &str, source_dir: &Path) -> anyhow::Result<ResolvedProfile> {
    if name == "all" {
        let mut skills = Vec::new();
        for group in super::skill_groups(source_dir) {
            for skill in super::skills_in_group(source_dir, &group) {
                skills.push((group.clone(), skill));
            }
        }
        return Ok(ResolvedProfile {
            name: "all".to_string(),
            description: "All available skills".to_string(),
            skills,
        });
    }

    let profiles = available_profiles(source_dir)?;
    let def = profiles.get(name).ok_or_else(|| {
        let available: Vec<_> = profiles
            .keys()
            .chain(std::iter::once(&"all".to_string()))
            .cloned()
            .collect();
        anyhow::anyhow!(
            "Unknown profile '{}'. Available: {}",
            name,
            available.join(", ")
        )
    })?;

    let mut skills = Vec::new();

    for spec in &def.skills {
        let mut components = spec.split('/');
        let group = components.next().unwrap_or_default();
        let skill_name = components.next().unwrap_or_default();
        if group.is_empty() || skill_name.is_empty() || components.next().is_some() {
            anyhow::bail!(
                "Invalid skill '{}' in profile '{}': expected exactly group/name",
                spec,
                name
            );
        }
        crate::util::validate_name(group)
            .with_context(|| format!("Invalid group in profile '{}' skill '{}'", name, spec))?;
        crate::util::validate_name(skill_name)
            .with_context(|| format!("Invalid name in profile '{}' skill '{}'", name, spec))?;

        let pair = (group.to_string(), skill_name.to_string());
        if !skills.contains(&pair) {
            skills.push(pair);
        }
    }

    for group in &def.groups {
        crate::util::validate_name(group)
            .with_context(|| format!("Invalid group '{}' in profile '{}'", group, name))?;
        for skill in super::skills_in_group(source_dir, group) {
            crate::util::validate_name(&skill).with_context(|| {
                format!(
                    "Invalid skill name '{}' discovered for group '{}' in profile '{}'",
                    skill, group, name
                )
            })?;
            let pair = (group.clone(), skill);
            if !skills.contains(&pair) {
                skills.push(pair);
            }
        }
    }

    Ok(ResolvedProfile {
        name: name.to_string(),
        description: def.description.clone(),
        skills,
    })
}

pub fn list_profiles(source_dir: &Path) -> Result<Vec<(String, String, usize)>> {
    list_profiles_inner(source_dir, true)
}

/// List profiles without builtins — for remote repos that have their own profiles.yml.
pub fn list_profiles_remote(source_dir: &Path) -> Result<Vec<(String, String, usize)>> {
    list_profiles_inner(source_dir, false)
}

fn list_profiles_inner(
    source_dir: &Path,
    include_builtins: bool,
) -> Result<Vec<(String, String, usize)>> {
    let profiles = available_profiles_with_builtins(source_dir, include_builtins)?;
    let mut result = Vec::with_capacity(profiles.len() + 1);
    for (name, def) in profiles {
        let count = resolve_profile(&name, source_dir)?.skills.len();
        result.push((name, def.description, count));
    }

    let all_count: usize = super::skill_groups(source_dir)
        .iter()
        .map(|g| super::skills_in_group(source_dir, g).len())
        .sum();
    result.push((
        "all".to_string(),
        "All available skills".to_string(),
        all_count,
    ));

    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_profiles_file_does_not_fall_back_to_builtin_profile() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("profiles.yml");
        std::fs::write(&path, "core: [\n").unwrap();

        let error = match resolve_profile("core", temp.path()) {
            Ok(_) => panic!("malformed profiles.yml unexpectedly resolved the builtin profile"),
            Err(error) => error,
        };

        let message = format!("{error:#}");
        assert!(message.contains("Invalid profiles YAML"));
        assert!(message.contains(&path.display().to_string()));
    }

    #[test]
    fn missing_profiles_file_preserves_builtin_profile() {
        let temp = tempfile::TempDir::new().unwrap();

        let profile = resolve_profile("core", temp.path()).unwrap();

        assert_eq!(profile.name, "core");
        assert!(!profile.skills.is_empty());
    }

    #[test]
    fn profile_skills_require_exact_validated_group_name_pairs() {
        for invalid in [
            "skill",
            "/skill",
            "group/",
            "group//skill",
            "group/skill/extra",
            "../skill",
            "group/../skill",
        ] {
            let temp = tempfile::TempDir::new().unwrap();
            std::fs::write(
                temp.path().join("profiles.yml"),
                format!("test:\n  skills:\n    - '{invalid}'\n"),
            )
            .unwrap();

            let error = match resolve_profile("test", temp.path()) {
                Ok(_) => panic!("invalid profile skill unexpectedly resolved: {invalid}"),
                Err(error) => error,
            };
            assert!(
                format!("{error:#}").contains("profile 'test'"),
                "missing profile context for {invalid}: {error:#}"
            );
        }
    }

    #[test]
    fn valid_profile_pairs_and_group_expansion_are_preserved() {
        let temp = tempfile::TempDir::new().unwrap();
        for skill in ["direct", "expanded"] {
            let path = temp.path().join("group").join(skill);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("SKILL.md"), "skill").unwrap();
        }
        std::fs::write(
            temp.path().join("profiles.yml"),
            "test:\n  skills:\n    - group/direct\n  groups:\n    - group\n",
        )
        .unwrap();

        let profile = resolve_profile("test", temp.path()).unwrap();
        assert_eq!(
            profile.skills,
            vec![
                ("group".to_string(), "direct".to_string()),
                ("group".to_string(), "expanded".to_string()),
            ]
        );
    }
}

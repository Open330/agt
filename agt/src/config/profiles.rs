use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileDef {
    #[serde(default)]
    pub description: String,
    /// Profiles from the same source whose skills come first: `extends: core`
    /// or `extends: [core, dev]`.
    #[serde(default, deserialize_with = "one_or_many")]
    pub extends: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

fn one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(name) => vec![name],
        OneOrMany::Many(names) => names,
    })
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
            extends: vec![],
            skills: vec![
                "development/git-commit-pr".into(),
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

fn load_profiles_file(source_dir: &Path) -> Option<BTreeMap<String, ProfileDef>> {
    let mut merged = BTreeMap::new();

    // Try profiles.yml first (canonical name)
    let canonical = source_dir.join("profiles.yml");
    if let Ok(content) = std::fs::read_to_string(&canonical) {
        if let Ok(profiles) = serde_yaml::from_str::<BTreeMap<String, ProfileDef>>(&content) {
            merged.extend(profiles);
        }
    }

    // Also scan all *.yml files at root (repos may split profiles across files).
    // Sorted so that a name defined twice resolves the same way on every machine.
    if let Ok(entries) = std::fs::read_dir(source_dir) {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|e| e.to_str()) == Some("yml")
                    && path.file_name().unwrap_or_default() != "profiles.yml"
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(profiles) = serde_yaml::from_str::<BTreeMap<String, ProfileDef>>(&content)
                {
                    merged.extend(profiles);
                }
            }
        }
    }

    if merged.is_empty() {
        None
    } else {
        Some(merged)
    }
}

fn available_profiles(source_dir: &Path) -> BTreeMap<String, ProfileDef> {
    available_profiles_with_builtins(source_dir, true)
}

fn available_profiles_with_builtins(
    source_dir: &Path,
    include_builtins: bool,
) -> BTreeMap<String, ProfileDef> {
    let mut profiles = if include_builtins {
        builtin_profiles()
    } else {
        BTreeMap::new()
    };
    if let Some(file_profiles) = load_profiles_file(source_dir) {
        for (name, def) in file_profiles {
            profiles.insert(name, def);
        }
    }
    profiles
}

/// Resolve a profile name, a comma-separated list (`core,dev`), or `all` into
/// an ordered, de-duplicated skill list. `extends` is followed recursively.
pub fn resolve_profile(name: &str, source_dir: &Path) -> anyhow::Result<ResolvedProfile> {
    let profiles = available_profiles(source_dir);
    let names: Vec<&str> = name
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        anyhow::bail!("Profile name is empty");
    }

    let mut skills = Vec::new();
    let mut descriptions = Vec::new();
    for profile in &names {
        let mut chain = Vec::new();
        collect_profile_skills(profile, &profiles, source_dir, &mut chain, &mut skills)?;
        descriptions.push(profile_description(profile, &profiles));
    }

    Ok(ResolvedProfile {
        name: names.join(","),
        description: descriptions.join(" + "),
        skills,
    })
}

fn profile_description(name: &str, profiles: &BTreeMap<String, ProfileDef>) -> String {
    match profiles.get(name) {
        Some(def) => def.description.clone(),
        None => "All available skills".to_string(),
    }
}

fn push_unique(skills: &mut Vec<(String, String)>, pair: (String, String)) {
    if !skills.contains(&pair) {
        skills.push(pair);
    }
}

fn collect_profile_skills(
    name: &str,
    profiles: &BTreeMap<String, ProfileDef>,
    source_dir: &Path,
    chain: &mut Vec<String>,
    skills: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    if chain.iter().any(|n| n == name) {
        anyhow::bail!(
            "Profile 'extends' cycle: {} -> {}",
            chain.join(" -> "),
            name
        );
    }

    if name == "all" && !profiles.contains_key("all") {
        for group in super::skill_groups(source_dir) {
            for skill in super::skills_in_group(source_dir, &group) {
                push_unique(skills, (group.clone(), skill));
            }
        }
        return Ok(());
    }

    let def = profiles.get(name).ok_or_else(|| {
        let available: Vec<_> = profiles
            .keys()
            .chain(std::iter::once(&"all".to_string()))
            .cloned()
            .collect();
        match chain.last() {
            Some(parent) => anyhow::anyhow!(
                "Profile '{}' extends unknown profile '{}'. Available: {}",
                parent,
                name,
                available.join(", ")
            ),
            None => anyhow::anyhow!(
                "Unknown profile '{}'. Available: {}",
                name,
                available.join(", ")
            ),
        }
    })?;

    chain.push(name.to_string());
    for parent in &def.extends {
        collect_profile_skills(parent, profiles, source_dir, chain, skills)?;
    }
    chain.pop();

    for spec in &def.skills {
        if let Some((group, skill_name)) = spec.split_once('/') {
            push_unique(skills, (group.to_string(), skill_name.to_string()));
        }
    }

    for group in &def.groups {
        for skill in super::skills_in_group(source_dir, group) {
            push_unique(skills, (group.clone(), skill));
        }
    }

    Ok(())
}

pub fn list_profiles(source_dir: &Path) -> Vec<(String, String, usize)> {
    list_profiles_inner(source_dir, true)
}

/// List profiles without builtins — for remote repos that have their own profiles.yml.
pub fn list_profiles_remote(source_dir: &Path) -> Vec<(String, String, usize)> {
    list_profiles_inner(source_dir, false)
}

fn list_profiles_inner(
    source_dir: &Path,
    include_builtins: bool,
) -> Vec<(String, String, usize)> {
    let profiles = available_profiles_with_builtins(source_dir, include_builtins);
    let mut result: Vec<(String, String, usize)> = profiles
        .into_iter()
        .map(|(name, def)| {
            let count = resolve_profile(&name, source_dir)
                .map(|r| r.skills.len())
                .unwrap_or(0);
            (name, def.description, count)
        })
        .collect();

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
    result
}

#[cfg(test)]
mod tests {
    use super::resolve_profile;
    use std::fs;
    use std::path::Path;

    fn source(profiles_yml: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (group, skill) in [("dev", "a"), ("dev", "b"), ("ops", "c")] {
            let dir = tmp.path().join(group).join(skill);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
        }
        fs::write(tmp.path().join("profiles.yml"), profiles_yml).unwrap();
        tmp
    }

    fn names(dir: &Path, profile: &str) -> Vec<String> {
        resolve_profile(profile, dir)
            .unwrap()
            .skills
            .into_iter()
            .map(|(g, s)| format!("{g}/{s}"))
            .collect()
    }

    #[test]
    fn extends_puts_parent_skills_first() {
        let src = source("base:\n  skills: [dev/b]\nfull:\n  extends: base\n  groups: [ops]\n");
        assert_eq!(names(src.path(), "full"), ["dev/b", "ops/c"]);
    }

    #[test]
    fn extends_accepts_a_list_and_dedupes() {
        let src =
            source("x:\n  skills: [dev/a]\ny:\n  skills: [dev/a, dev/b]\nz:\n  extends: [x, y]\n");
        assert_eq!(names(src.path(), "z"), ["dev/a", "dev/b"]);
    }

    #[test]
    fn comma_list_unions_profiles_in_order() {
        let src = source("x:\n  skills: [ops/c]\ny:\n  skills: [dev/a, ops/c]\n");
        let resolved = resolve_profile("x, y", src.path()).unwrap();
        assert_eq!(resolved.name, "x,y");
        assert_eq!(names(src.path(), "x,y"), ["ops/c", "dev/a"]);
    }

    #[test]
    fn extends_cycle_is_an_error() {
        let src = source("x:\n  extends: y\ny:\n  extends: x\n");
        let err = resolve_profile("x", src.path()).err().unwrap().to_string();
        assert!(err.contains("cycle"), "{err}");
    }

    #[test]
    fn unknown_parent_names_the_child() {
        let src = source("x:\n  extends: nope\n");
        let err = resolve_profile("x", src.path()).err().unwrap().to_string();
        assert!(err.contains("'x' extends unknown profile 'nope'"), "{err}");
    }

    #[test]
    fn extends_all_includes_every_skill() {
        let src = source("x:\n  extends: all\n");
        assert_eq!(names(src.path(), "x"), ["dev/a", "dev/b", "ops/c"]);
    }
}

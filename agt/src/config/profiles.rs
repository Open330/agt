use anyhow::{Context, Result};
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

fn one_or_many<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
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

    // Also scan all *.yml files at root (repos may split profiles across files).
    // Sorted so that a name defined twice resolves the same way on every machine.
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
    let mut paths = Vec::new();
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
            paths.push(path);
        }
    }
    paths.sort();
    for path in paths {
        if let Some(profiles) = read_profiles(&path)? {
            merged.extend(profiles);
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

/// Resolve a profile name, a comma-separated list (`core,dev`), or `all` into
/// an ordered, de-duplicated skill list. `extends` is followed recursively.
pub fn resolve_profile(name: &str, source_dir: &Path) -> anyhow::Result<ResolvedProfile> {
    let names: Vec<&str> = name
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        anyhow::bail!("Profile name is empty");
    }
    // `all` alone needs no profiles file, so a broken one does not block it.
    let profiles = if names == ["all"] {
        BTreeMap::new()
    } else {
        available_profiles(source_dir)?
    };

    let mut skills = Vec::new();
    let mut descriptions = Vec::new();
    let mut done = std::collections::HashSet::new();
    for profile in &names {
        let mut chain = Vec::new();
        collect_profile_skills(
            profile,
            &profiles,
            source_dir,
            &mut chain,
            &mut done,
            &mut skills,
        )?;
        descriptions.push(match profiles.get(*profile) {
            Some(def) => def.description.clone(),
            None => "All available skills".to_string(),
        });
    }

    Ok(ResolvedProfile {
        name: names.join(","),
        description: descriptions.join(" + "),
        skills,
    })
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
    done: &mut std::collections::HashSet<String>,
    skills: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    if chain.iter().any(|n| n == name) {
        anyhow::bail!(
            "Profile 'extends' cycle: {} -> {}",
            chain.join(" -> "),
            name
        );
    }
    // Each profile contributes once; a diamond of `extends` would otherwise
    // be walked once per path, which grows exponentially with depth.
    if !done.insert(name.to_string()) {
        return Ok(());
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
        collect_profile_skills(parent, profiles, source_dir, chain, done, skills)?;
    }
    chain.pop();

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
        push_unique(skills, (group.to_string(), skill_name.to_string()));
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
            push_unique(skills, (group.clone(), skill));
        }
    }

    Ok(())
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
    fn deep_extends_diamond_resolves_quickly() {
        let mut yml = String::from("p40:\n  skills: [dev/a]\n");
        for i in 0..40 {
            yml.push_str(&format!("p{i}:\n  extends: [p{n}, p{n}]\n", n = i + 1));
        }
        let src = source(&yml);
        let started = std::time::Instant::now();
        assert_eq!(names(src.path(), "p0"), ["dev/a"]);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
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

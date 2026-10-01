use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{resolve_home, resolve_profile, SkillAgent};

/// `~/.config/agt/layers.toml`: which skill sources and profiles are active
/// where. Private to the machine; never part of a skills repository.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayersConfig {
    /// Source name -> local skills repository path.
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub stack: BTreeMap<String, StackDef>,
    #[serde(default)]
    pub target: Vec<TargetDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackDef {
    /// Stack whose layers come first.
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub layers: Vec<LayerDef>,
    /// Sources whose `[[setup.copy]]` rules run when this stack is applied to
    /// the global target. Ignored for directory targets.
    #[serde(default, rename = "static")]
    pub static_sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerDef {
    pub source: String,
    /// Profile name or comma list, resolved in the source repository.
    #[serde(default)]
    pub profile: Option<String>,
    /// Individual `group/skill` entries, for a machine-local pick that does
    /// not belong in the source repository's profiles. Added after `profile`.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Let this layer replace a same-named skill from an earlier layer.
    #[serde(default, rename = "override")]
    pub override_earlier: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDef {
    /// `global`, or a directory whose git repositories get the stack.
    pub path: String,
    pub stack: String,
    #[serde(default)]
    pub agent: SkillAgent,
}

/// One skill the stack wants, and the layer that wants it.
#[derive(Debug, Clone, PartialEq)]
pub struct DesiredSkill {
    pub name: String,
    pub layer: String,
    pub source_dir: PathBuf,
    pub group: String,
}

impl DesiredSkill {
    pub fn skill_path(&self) -> PathBuf {
        self.source_dir.join(&self.group).join(&self.name)
    }
}

pub fn layers_config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("AGT_LAYERS").filter(|v| !v.is_empty()) {
        return resolve_home(&path.to_string_lossy());
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config/agt/layers.toml")
}

impl LayersConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read layer config {}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("Invalid layer config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        for target in &self.target {
            if !self.stack.contains_key(&target.stack) {
                bail!(
                    "Target '{}' uses unknown stack '{}'",
                    target.path,
                    target.stack
                );
            }
        }
        Ok(())
    }

    pub fn source_dir(&self, name: &str) -> Result<PathBuf> {
        let path = self
            .sources
            .get(name)
            .with_context(|| format!("Unknown source '{name}' (add it under [sources])"))?;
        let dir = resolve_home(path);
        if !dir.is_dir() {
            bail!("Source '{}' not found at {}", name, dir.display());
        }
        Ok(dir)
    }

    /// Stacks from the root of the `extends` chain down to `name`.
    fn stack_chain(&self, name: &str) -> Result<Vec<&StackDef>> {
        let mut chain = Vec::new();
        let mut seen = Vec::new();
        let mut current = Some(name.to_string());
        while let Some(stack_name) = current {
            if seen.contains(&stack_name) {
                bail!(
                    "Stack 'extends' cycle: {} -> {}",
                    seen.join(" -> "),
                    stack_name
                );
            }
            let def = self
                .stack
                .get(&stack_name)
                .with_context(|| format!("Unknown stack '{stack_name}'"))?;
            seen.push(stack_name);
            chain.push(def);
            current = def.extends.clone();
        }
        chain.reverse();
        Ok(chain)
    }

    pub fn layers(&self, stack: &str) -> Result<Vec<LayerDef>> {
        Ok(self
            .stack_chain(stack)?
            .into_iter()
            .flat_map(|def| def.layers.iter().cloned())
            .collect())
    }

    pub fn static_sources(&self, stack: &str) -> Result<Vec<String>> {
        let mut sources = Vec::new();
        for def in self.stack_chain(stack)? {
            for source in &def.static_sources {
                if !sources.contains(source) {
                    sources.push(source.clone());
                }
            }
        }
        Ok(sources)
    }

    /// Every skill the stack installs, in layer order. Two layers providing
    /// the same skill name is an error unless the later one sets `override`.
    pub fn desired_skills(&self, stack: &str) -> Result<Vec<DesiredSkill>> {
        let mut desired: Vec<DesiredSkill> = Vec::new();
        for layer in self.layers(stack)? {
            let source_dir = self.source_dir(&layer.source)?;
            let label = match &layer.profile {
                Some(profile) => format!("{}:{}", layer.source, profile),
                None => format!("{}:skills", layer.source),
            };
            let mut pairs = match &layer.profile {
                Some(profile) => {
                    resolve_profile(profile, &source_dir)
                        .with_context(|| format!("Layer {label}"))?
                        .skills
                }
                None if layer.skills.is_empty() => bail!(
                    "Layer from source '{}' needs a `profile` or `skills`",
                    layer.source
                ),
                None => Vec::new(),
            };
            for spec in &layer.skills {
                let (group, name) = spec.split_once('/').with_context(|| {
                    format!("Layer {label}: skill '{spec}' must be `group/skill`")
                })?;
                let pair = (group.to_string(), name.to_string());
                if !pairs.contains(&pair) {
                    pairs.push(pair);
                }
            }
            for (group, name) in pairs {
                let skill = DesiredSkill {
                    name: name.clone(),
                    layer: label.clone(),
                    source_dir: source_dir.clone(),
                    group,
                };
                if !skill.skill_path().join("SKILL.md").exists() {
                    bail!(
                        "Layer {} lists '{}/{}', which has no SKILL.md in {}",
                        label,
                        skill.group,
                        name,
                        source_dir.display()
                    );
                }
                match desired.iter().position(|d| d.name == name) {
                    None => desired.push(skill),
                    Some(i) if desired[i].skill_path() == skill.skill_path() => {}
                    Some(i) if layer.override_earlier => desired[i] = skill,
                    Some(i) => bail!(
                        "Skill '{}' comes from both {} and {}; set `override = true` on the later layer to replace it",
                        name,
                        desired[i].layer,
                        label
                    ),
                }
            }
        }
        Ok(desired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_with(dir: &Path, skills: &[&str], profiles: &str) {
        for spec in skills {
            let path = dir.join(spec);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
        }
        fs::write(dir.join("profiles.yml"), profiles).unwrap();
    }

    fn config(toml_src: &str) -> LayersConfig {
        let config: LayersConfig = toml::from_str(toml_src).unwrap();
        config.validate().unwrap();
        config
    }

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        source_with(
            &tmp.path().join("personal"),
            &["dev/commit", "dev/review"],
            "core:\n  skills: [dev/commit]\ndev:\n  groups: [dev]\n",
        );
        source_with(
            &tmp.path().join("team"),
            &["work/deploy", "work/commit"],
            "team:\n  skills: [work/deploy]\nclash:\n  skills: [work/commit]\n",
        );
        tmp
    }

    fn names(skills: &[DesiredSkill]) -> Vec<String> {
        skills
            .iter()
            .map(|s| format!("{}={}", s.name, s.layer))
            .collect()
    }

    fn base_toml(root: &Path, team_layer: &str) -> String {
        format!(
            "[sources]\npersonal = \"{p}\"\nteam = \"{t}\"\n\n\
             [stack.base]\nlayers = [{{ source = \"personal\", profile = \"core\" }}]\n\n\
             [stack.work]\nextends = \"base\"\nlayers = [{team_layer}]\n",
            p = root.join("personal").display(),
            t = root.join("team").display(),
        )
    }

    #[test]
    fn extended_stack_lists_parent_layers_first() {
        let tmp = fixture();
        let cfg = config(&base_toml(
            tmp.path(),
            "{ source = \"team\", profile = \"team\" }",
        ));
        assert_eq!(
            names(&cfg.desired_skills("work").unwrap()),
            ["commit=personal:core", "deploy=team:team"]
        );
    }

    #[test]
    fn same_name_from_two_layers_needs_override() {
        let tmp = fixture();
        let cfg = config(&base_toml(
            tmp.path(),
            "{ source = \"team\", profile = \"clash\" }",
        ));
        let err = cfg.desired_skills("work").err().unwrap().to_string();
        assert!(err.contains("override = true"), "{err}");

        let cfg = config(&base_toml(
            tmp.path(),
            "{ source = \"team\", profile = \"clash\", override = true }",
        ));
        assert_eq!(
            names(&cfg.desired_skills("work").unwrap()),
            ["commit=team:clash"]
        );
    }

    #[test]
    fn layer_can_pick_individual_skills() {
        let tmp = fixture();
        let cfg = config(&base_toml(
            tmp.path(),
            "{ source = \"personal\", skills = [\"dev/review\"] }",
        ));
        assert_eq!(
            names(&cfg.desired_skills("work").unwrap()),
            ["commit=personal:core", "review=personal:skills"]
        );
    }

    #[test]
    fn layer_without_profile_or_skills_is_rejected() {
        let tmp = fixture();
        let cfg = config(&base_toml(tmp.path(), "{ source = \"personal\" }"));
        let err = cfg.desired_skills("work").err().unwrap().to_string();
        assert!(err.contains("needs a `profile` or `skills`"), "{err}");
    }

    #[test]
    fn unknown_stack_in_target_is_rejected() {
        let parsed: LayersConfig =
            toml::from_str("[[target]]\npath = \"global\"\nstack = \"nope\"\n").unwrap();
        assert!(parsed.validate().is_err());
    }

    #[test]
    fn stack_cycle_is_an_error() {
        let cfg = config("[stack.a]\nextends = \"b\"\n[stack.b]\nextends = \"a\"\n");
        assert!(cfg.layers("a").err().unwrap().to_string().contains("cycle"));
    }

    #[test]
    fn static_sources_inherit_through_extends() {
        let cfg = config(
            "[stack.a]\nstatic = [\"personal\"]\n[stack.b]\nextends = \"a\"\nstatic = [\"team\", \"personal\"]\n",
        );
        assert_eq!(cfg.static_sources("b").unwrap(), ["personal", "team"]);
    }
}

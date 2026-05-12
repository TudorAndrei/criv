use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::util::read_to_string;
use crate::{CrivError, Result};

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) docs_dir: String,
    pub(crate) adr_dir: String,
    pub(crate) source_roots: Vec<String>,
    pub(crate) source_exclude: Vec<String>,
    pub(crate) languages: Vec<String>,
    pub(crate) source_index: bool,
    pub(crate) notes_index: String,
    pub(crate) embeddings: bool,
    pub(crate) enforce_stages: Vec<String>,
    pub(crate) obsidian_plugin: bool,
    pub(crate) patterns: BTreeSet<String>,
    pub(crate) pattern_defs: BTreeMap<String, PatternConfig>,
}

#[derive(Debug, Clone)]
pub(crate) struct PatternConfig {
    pub(crate) language: Option<String>,
    pub(crate) pattern: Option<String>,
    pub(crate) rule: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            docs_dir: "docs".into(),
            adr_dir: "adr".into(),
            source_roots: vec!["src".into(), "lib".into()],
            source_exclude: vec!["**/target/**".into(), "**/node_modules/**".into()],
            languages: Vec::new(),
            source_index: true,
            notes_index: "memory".into(),
            embeddings: false,
            enforce_stages: vec!["commit".into(), "push".into(), "ci".into()],
            obsidian_plugin: true,
            patterns: BTreeSet::new(),
            pattern_defs: BTreeMap::new(),
        }
    }
}

impl Config {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let path = root.join("criv.toml");
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = read_to_string(&path)?;
        let raw: RawConfig = toml::from_str(&contents)
            .map_err(|err| CrivError::new(format!("failed to parse criv.toml: {err}")))?;
        Ok(raw.into_config())
    }

    pub(crate) fn docs_path(&self, root: &Path) -> PathBuf {
        root.join(&self.docs_dir)
    }

    pub(crate) fn source_root_paths(&self, root: &Path) -> Vec<PathBuf> {
        self.source_roots
            .iter()
            .map(|source_root| root.join(source_root))
            .filter(|path| path.exists())
            .collect()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawConfig {
    vault: RawVault,
    source: RawSource,
    index: RawIndex,
    enforce: RawEnforce,
    obsidian: RawObsidian,
    patterns: BTreeMap<String, toml::Value>,
}

impl RawConfig {
    fn into_config(self) -> Config {
        let defaults = Config::default();
        Config {
            docs_dir: self.vault.docs.unwrap_or(defaults.docs_dir),
            adr_dir: self.vault.adr.unwrap_or(defaults.adr_dir),
            source_roots: self.source.roots.unwrap_or(defaults.source_roots),
            source_exclude: self.source.exclude.unwrap_or(defaults.source_exclude),
            languages: self.source.languages.unwrap_or(defaults.languages),
            source_index: self.index.source.unwrap_or(defaults.source_index),
            notes_index: self.index.notes.unwrap_or(defaults.notes_index),
            embeddings: self.index.embeddings.unwrap_or(defaults.embeddings),
            enforce_stages: self.enforce.stages.unwrap_or(defaults.enforce_stages),
            obsidian_plugin: self.obsidian.plugin.unwrap_or(defaults.obsidian_plugin),
            patterns: self.patterns.keys().cloned().collect(),
            pattern_defs: self
                .patterns
                .into_iter()
                .map(|(id, value)| (id, PatternConfig::from_toml(value)))
                .collect(),
        }
    }
}

impl PatternConfig {
    fn from_toml(value: toml::Value) -> Self {
        Self {
            language: value
                .get("language")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            pattern: value
                .get("pattern")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            rule: value
                .get("rule")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        }
    }

    pub(crate) fn lexical_pattern(&self) -> Option<&str> {
        self.pattern
            .as_deref()
            .or_else(|| self.rule.as_deref().and_then(extract_rule_pattern))
    }
}

fn extract_rule_pattern(rule: &str) -> Option<&str> {
    rule.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("pattern:").map(str::trim))
}

#[derive(Debug, Default, Deserialize)]
struct RawVault {
    docs: Option<String>,
    adr: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSource {
    roots: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    languages: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawIndex {
    source: Option<bool>,
    notes: Option<String>,
    embeddings: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawEnforce {
    stages: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawObsidian {
    plugin: Option<bool>,
}

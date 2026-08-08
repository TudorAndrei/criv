use criv_state_wire::StateDocument;
use serde::Serialize;

use crate::{CrivError, Result};

pub(crate) fn default_config() -> Result<String> {
    Ok(DEFAULT_CONFIG.to_string())
}

pub(crate) fn default_state() -> Result<String> {
    json_pretty(&StateDocument::default(), ".criv/state.json")
}

pub(crate) fn adr_readme() -> Result<String> {
    let frontmatter = serde_norway::to_string(&AdrReadmeFrontmatter::default()).map_err(|err| {
        CrivError::new(format!(
            "failed to serialize docs/adr/README.md frontmatter: {err}"
        ))
    })?;
    Ok(format!(
        "---\n{}---\n\n# Architectural Decisions\n\nAccepted decisions live in this directory as MADR-style notes named `NNNN-kebab-title.md`.\n",
        frontmatter
    ))
}

fn json_pretty(value: &impl Serialize, label: &str) -> Result<String> {
    let mut json = serde_json::to_string_pretty(value)
        .map_err(|err| CrivError::new(format!("failed to serialize {label}: {err}")))?;
    json.push('\n');
    Ok(json)
}

#[cfg(test)]
#[derive(Debug, Serialize)]
struct DefaultConfig {
    vault: VaultConfig,
    source: SourceConfig,
    index: IndexConfig,
    state: StateConfig,
    enforce: EnforceConfig,
}

#[cfg(test)]
impl Default for DefaultConfig {
    fn default() -> Self {
        Self {
            vault: VaultConfig {
                docs: "docs",
                adr: "adr",
            },
            source: SourceConfig {
                roots: vec!["src", "lib"],
                exclude: vec!["**/target/**", "**/node_modules/**"],
            },
            index: IndexConfig { source: true },
            state: StateConfig { keep: 20 },
            enforce: EnforceConfig {
                stages: vec!["commit", "push", "ci"],
            },
        }
    }
}

#[cfg(test)]
#[derive(Debug, Serialize)]
struct VaultConfig {
    docs: &'static str,
    adr: &'static str,
}

#[cfg(test)]
#[derive(Debug, Serialize)]
struct SourceConfig {
    roots: Vec<&'static str>,
    exclude: Vec<&'static str>,
}

#[cfg(test)]
#[derive(Debug, Serialize)]
struct IndexConfig {
    source: bool,
}

#[cfg(test)]
#[derive(Debug, Serialize)]
struct StateConfig {
    keep: usize,
}

#[cfg(test)]
#[derive(Debug, Serialize)]
struct EnforceConfig {
    stages: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AdrReadmeFrontmatter {
    id: &'static str,
    kind: &'static str,
    tags: Vec<&'static str>,
}

impl Default for AdrReadmeFrontmatter {
    fn default() -> Self {
        Self {
            id: "ADR-README",
            kind: "doc",
            tags: vec!["criv"],
        }
    }
}

const DEFAULT_CONFIG: &str = include_str!("fixtures/criv.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_fixture_matches_typed_defaults() {
        let fixture: toml::Value = toml::from_str(DEFAULT_CONFIG).unwrap();
        let defaults = toml::Value::try_from(DefaultConfig::default()).unwrap();

        assert_eq!(fixture, defaults);
    }
}

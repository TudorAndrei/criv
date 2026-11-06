use criv_state_wire::{StateDocument, layout};
use serde::Serialize;

use crate::{CrivError, Result};

pub const DEFAULT_CONFIG: &str = include_str!("fixtures/criv.toml");

pub fn default_state() -> Result<String> {
    let mut json = serde_json::to_string_pretty(&StateDocument::default())
        .map_err(|err| CrivError::new(format!("failed to serialize {}: {err}", layout::STATE)))?;
    json.push('\n');
    Ok(json)
}

pub fn adr_readme() -> Result<String> {
    let frontmatter = serde_norway::to_string(&AdrReadmeFrontmatter::default()).map_err(|err| {
        CrivError::new(format!(
            "failed to serialize docs/adr/README.md frontmatter: {err}"
        ))
    })?;
    Ok(format!(
        "---\n{frontmatter}---\n\n# Architectural Decisions\n\nAccepted decisions live in this directory as MADR-style notes named `NNNN-kebab-title.md`.\n"
    ))
}

#[derive(Debug, Serialize)]
struct AdrReadmeFrontmatter {
    id: &'static str,
    kind: &'static str,
    title: &'static str,
    tags: Vec<&'static str>,
}

impl Default for AdrReadmeFrontmatter {
    fn default() -> Self {
        Self {
            id: "ADR-README",
            kind: "doc",
            title: "Architectural Decisions",
            tags: vec!["criv"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::Config;

    #[test]
    fn default_config_template_states_the_config_defaults() {
        let parsed = Config::parse(Some(DEFAULT_CONFIG)).unwrap();
        assert_eq!(parsed, Config::default());

        let template: toml::Value = toml::from_str(DEFAULT_CONFIG).unwrap();
        for (table, key) in [
            ("vault", "docs"),
            ("vault", "adr"),
            ("source", "roots"),
            ("source", "exclude"),
            ("index", "source"),
            ("state", "keep"),
            ("enforce", "stages"),
        ] {
            assert!(
                template
                    .get(table)
                    .and_then(|value| value.get(key))
                    .is_some(),
                "the template must state {table}.{key} rather than rely on the default"
            );
        }
    }
}

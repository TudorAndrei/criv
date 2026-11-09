use criv_state_wire::{StateDocument, layout};
use serde::Serialize;

use crate::{CrivError, Result};

pub const DEFAULT_CONFIG: &str = include_str!("fixtures/criv.toml");

pub fn default_state() -> Result<String> {
    criv_state_wire::encode(&StateDocument::default())
        .map_err(|err| CrivError::new(format!("failed to serialize {}: {err}", layout::STATE)))
}

pub fn adr_readme() -> Result<String> {
    let frontmatter = serde_norway::to_string(&AdrReadmeFrontmatter::default()).map_err(|err| {
        CrivError::new(format!(
            "failed to serialize the decision index frontmatter: {err}"
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

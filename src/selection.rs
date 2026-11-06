//! The repository files criv selects, for callers outside the crate that
//! observe selection through the same code paths as the CLI (ADR-0173).

use std::path::Path;

use crate::Result;
use crate::config::Config;
use crate::discovery::{self, SourceScope};
use crate::repository::RepositoryFiles;
use crate::source::SourceState;

/// Source files after the complete Source build: candidate discovery,
/// classification, hashing, and parsing.
///
/// # Errors
///
/// Returns an error when the configuration or a selected file cannot be read.
pub fn source_files(root: &Path) -> Result<Vec<String>> {
    let files = RepositoryFiles::open(root)?;
    let config = Config::load_from(&files)?;
    Ok(SourceState::refresh(&files, &config, None)?
        .paths()
        .to_vec())
}

/// Source candidates from the directory walk alone, before any file is opened.
///
/// # Errors
///
/// Returns an error when the configuration or the Source scope is invalid.
pub fn source_candidates(root: &Path) -> Result<Vec<String>> {
    let files = RepositoryFiles::open(root)?;
    let config = Config::load_from(&files)?;
    discovery::discover_source_candidates(&files, &SourceScope::new(&config)?)
}

/// Markdown notes in the configured vault.
///
/// # Errors
///
/// Returns an error when the configuration or the vault cannot be read.
pub fn vault_markdown(root: &Path) -> Result<Vec<String>> {
    let config = Config::load_from(&RepositoryFiles::open(root)?)?;
    Ok(discovery::discover_vault(root, config.docs_dir())?.markdown)
}

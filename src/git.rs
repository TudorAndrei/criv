//! The embedded Git boundary for production repository access.
//!
//! Callers use criv values and errors only. `git2` objects stay in this module
//! so a runtime subprocess alternative cannot grow beside the embedded backend.

use std::path::Path;

use crate::{CrivError, Result};

/// Reads a file from the tree selected by `reference` in the repository
/// discovered from `root`.
pub(crate) fn read_file_at_ref(root: &Path, reference: &str, path: &Path) -> Result<Vec<u8>> {
    let repository = git2::Repository::discover(root).map_err(|error| {
        CrivError::new(format!(
            "failed to open repository from `{}`: {error}",
            root.display()
        ))
    })?;
    let object = repository.revparse_single(reference).map_err(|error| {
        CrivError::new(format!("git ref `{reference}` does not resolve: {error}"))
    })?;
    let tree = object.peel_to_tree().map_err(|error| {
        CrivError::new(format!(
            "git ref `{reference}` does not resolve to a tree: {error}"
        ))
    })?;
    let entry = tree.get_path(path).map_err(|error| {
        CrivError::new(format!(
            "git ref `{reference}` does not contain `{}`: {error}",
            path.display()
        ))
    })?;
    let blob = entry
        .to_object(&repository)
        .and_then(|object| object.peel_to_blob())
        .map_err(|error| {
            CrivError::new(format!(
                "git ref `{reference}` does not contain blob `{}`: {error}",
                path.display()
            ))
        })?;

    Ok(blob.content().to_vec())
}

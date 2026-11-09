//! Reads of the files that discovery selected.

use std::path::Path;

use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

/// Reads one selected file by its repository-relative path.
pub fn read_selected(files: &RepositoryFiles, relative: &str) -> Result<Vec<u8>> {
    files
        .read(Path::new(relative))
        .map_err(|error| selected_read_error(relative, error))
}

/// Reads one selected file as UTF-8 text.
pub fn read_selected_text(files: &RepositoryFiles, relative: &str) -> Result<String> {
    String::from_utf8(read_selected(files, relative)?)
        .map_err(|error| selected_read_error(relative, error))
}

fn selected_read_error(relative: &str, error: impl std::fmt::Display) -> CrivError {
    CrivError::new(format!(
        "failed to read selected file `{relative}`: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_text_read_errors_report_the_repository_path() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::write(temp.path().join("docs/invalid.md"), [0xFF]).unwrap();
        let files = RepositoryFiles::open(temp.path()).unwrap();

        assert!(
            read_selected_text(&files, "docs/invalid.md")
                .unwrap_err()
                .to_string()
                .contains("failed to read selected file `docs/invalid.md`")
        );
    }
}

use std::path::{Component, Path};

use crate::util::read_file_in;
use crate::{CrivError, Result};

pub(crate) fn read_source_to_string(root: &Path, source_path: &str) -> Result<String> {
    let contents = read_source_bytes(root, source_path)?;
    Ok(String::from_utf8_lossy(&contents).into_owned())
}

pub(super) fn read_source_bytes(root: &Path, source_path: &str) -> Result<Vec<u8>> {
    validate_relative_source_path("source path", source_path)?;
    read_file_in(root, Path::new(source_path))
}

fn validate_relative_source_path(field: &str, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CrivError::new(format!("{field} must not be empty")));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(CrivError::new(format!(
            "{field} `{value}` must be relative to the criv vault root"
        )));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(CrivError::new(format!(
                    "{field} `{value}` must not contain parent-directory segments"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(CrivError::new(format!(
                    "{field} `{value}` must be relative to the criv vault root"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[cfg(windows)]
    use std::fs;

    #[test]
    fn rejects_absolute_and_parent_source_paths() {
        let temp = TempDir::new().unwrap();

        let absolute = if cfg!(windows) {
            "C:\\secret"
        } else {
            "/etc/passwd"
        };
        assert!(read_source_bytes(temp.path(), absolute).is_err());
        assert!(read_source_bytes(temp.path(), "../secret").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_a_junction_in_a_selected_source_path() {
        let repository = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("source.rs"), "pub fn source() {}\n").unwrap();
        junction::create(outside.path(), repository.path().join("src")).unwrap();

        assert!(read_source_bytes(repository.path(), "src/source.rs").is_err());
    }
}

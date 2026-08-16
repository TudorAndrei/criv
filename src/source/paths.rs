use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::{CrivError, Result};

fn checked_source_path(root: &Path, source_path: &str) -> Result<PathBuf> {
    validate_relative_source_path("source path", source_path)?;
    let mut current = root.to_path_buf();
    for component in Path::new(source_path).components() {
        if let Component::Normal(part) = component {
            current.push(part);
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || is_junction(&current) {
            return Err(CrivError::new(format!(
                "refusing to read linked source path `{source_path}`"
            )));
        }
    }
    Ok(current)
}

pub(crate) fn read_source_to_string(root: &Path, source_path: &str) -> Result<String> {
    let contents = read_source_bytes(root, source_path)?;
    Ok(String::from_utf8_lossy(&contents).into_owned())
}

pub(super) fn read_source_bytes(root: &Path, source_path: &str) -> Result<Vec<u8>> {
    let path = checked_source_path(root, source_path)?;
    Ok(fs::read(path)?)
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

#[cfg(windows)]
fn is_junction(path: &Path) -> bool {
    junction::exists(path).unwrap_or(false)
}

#[cfg(not(windows))]
fn is_junction(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rejects_absolute_and_parent_source_paths() {
        let temp = TempDir::new().unwrap();

        let absolute = if cfg!(windows) {
            "C:\\secret"
        } else {
            "/etc/passwd"
        };
        assert!(checked_source_path(temp.path(), absolute).is_err());
        assert!(checked_source_path(temp.path(), "../secret").is_err());
    }
}

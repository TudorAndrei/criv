use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourceRootKind {
    Directory,
    File,
}

pub(crate) fn source_root_kind(root: &Path, source_root: &str) -> Result<Option<SourceRootKind>> {
    validate_relative_source_path("source.roots", source_root)?;
    let path = root.join(source_root);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        return Err(CrivError::new(format!(
            "source root `{source_root}` must not be a symlink"
        )));
    }
    canonical_source_path(root, source_root)?;
    if metadata.is_file() {
        Ok(Some(SourceRootKind::File))
    } else if metadata.is_dir() {
        Ok(Some(SourceRootKind::Directory))
    } else {
        Ok(None)
    }
}

pub(crate) fn canonical_source_path(root: &Path, source_path: &str) -> Result<PathBuf> {
    validate_relative_source_path("source path", source_path)?;
    let canonical_root = root.canonicalize()?;
    let canonical_path = root.join(source_path).canonicalize()?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(CrivError::new(format!(
            "source path `{source_path}` resolves outside the criv vault root"
        )));
    }
    Ok(canonical_path)
}

pub(crate) fn read_source_to_string(root: &Path, source_path: &str) -> Result<String> {
    let path = canonical_source_path(root, source_path)?;
    Ok(fs::read_to_string(path)?)
}

pub(crate) fn source_metadata(root: &Path, source_path: &str) -> Result<fs::Metadata> {
    let path = canonical_source_path(root, source_path)?;
    Ok(fs::metadata(path)?)
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

    #[test]
    fn rejects_absolute_and_parent_source_paths() {
        let temp = TempDir::new().unwrap();

        let absolute = if cfg!(windows) {
            "C:\\secret"
        } else {
            "/etc/passwd"
        };
        assert!(canonical_source_path(temp.path(), absolute).is_err());
        assert!(canonical_source_path(temp.path(), "../secret").is_err());
    }
}

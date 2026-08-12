use std::fs;
use std::path::{Path, PathBuf};

use crate::git;
use crate::util::{file_permissions_in, remove_file_in, write_atomic_with_permissions_in};
use crate::{CrivError, Result};

pub(crate) struct Snapshot {
    index_tree: String,
    paths: Vec<CapturedPath>,
}

struct CapturedPath {
    path: String,
    contents: Option<String>,
    permissions: Option<fs::Permissions>,
}

impl Snapshot {
    pub(crate) fn capture(root: &Path, paths: &[String]) -> Result<Self> {
        Ok(Self {
            index_tree: git::index_tree(root)?,
            paths: paths
                .iter()
                .map(|path| CapturedPath::capture(root, path))
                .collect::<Result<Vec<_>>>()?,
        })
    }

    pub(crate) fn rollback(self, root: &Path) -> Vec<String> {
        let mut errors = Vec::new();
        if let Err(error) = git::restore_index_tree(root, &self.index_tree) {
            errors.push(error.to_string());
        }
        for path in self.paths {
            if let Err(error) = path.restore(root) {
                errors.push(error.to_string());
            }
        }
        errors
    }
}

impl CapturedPath {
    fn capture(root: &Path, path: &str) -> Result<Self> {
        let relative = Path::new(path);
        let absolute = root.join(relative);
        match fs::symlink_metadata(&absolute) {
            Ok(_) => {
                let permissions = file_permissions_in(root, relative)?;
                Ok(Self {
                    path: path.to_string(),
                    contents: Some(fs::read_to_string(absolute)?),
                    permissions: Some(permissions),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                path: path.to_string(),
                contents: None,
                permissions: None,
            }),
            Err(error) => Err(error.into()),
        }
    }

    fn restore(self, root: &Path) -> Result<()> {
        let relative = PathBuf::from(&self.path);
        match (self.contents, self.permissions) {
            (Some(contents), Some(permissions)) => write_atomic_with_permissions_in(
                root,
                Path::new("."),
                &relative,
                &contents,
                permissions,
            ),
            (None, None) => match fs::symlink_metadata(root.join(&relative)) {
                Ok(_) => remove_file_in(root, Path::new("."), &relative),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            },
            _ => Err(CrivError::new(format!(
                "cannot restore incomplete snapshot for `{}`",
                self.path
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::Snapshot;

    #[test]
    fn rollback_restores_the_index_existing_files_and_absent_files() {
        let temp = repository();
        let root = temp.path();
        let existing = "docs/existing.md".to_string();
        let receipt = ".criv/reconcile.json".to_string();
        fs::create_dir_all(root.join(".criv")).unwrap();
        fs::write(root.join(&existing), "staged\n").unwrap();
        #[cfg(unix)]
        set_mode(&root.join(&existing), 0o640);
        git(root, &["add", &existing]);
        let index_before = crate::git::index_tree(root).unwrap();

        let snapshot = Snapshot::capture(root, &[existing.clone(), receipt.clone()]).unwrap();
        fs::write(root.join(&existing), "changed\n").unwrap();
        #[cfg(unix)]
        set_mode(&root.join(&existing), 0o755);
        fs::write(root.join(&receipt), "new receipt\n").unwrap();
        git(root, &["add", "-A"]);

        assert!(snapshot.rollback(root).is_empty());
        assert_eq!(fs::read_to_string(root.join(existing)).unwrap(), "staged\n");
        assert!(!root.join(receipt).exists());
        assert_eq!(crate::git::index_tree(root).unwrap(), index_before);
        #[cfg(unix)]
        assert_eq!(mode(&root.join("docs/existing.md")), 0o640);
    }

    #[test]
    fn rollback_continues_after_one_path_cannot_be_restored() {
        let temp = repository();
        let root = temp.path();
        let blocked = "docs/blocked.md".to_string();
        let later = "docs/later.md".to_string();
        fs::write(root.join(&blocked), "blocked before\n").unwrap();
        fs::write(root.join(&later), "later before\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "test: add rollback paths"]);
        let snapshot = Snapshot::capture(root, &[blocked.clone(), later.clone()]).unwrap();

        fs::remove_file(root.join(&blocked)).unwrap();
        fs::create_dir(root.join(&blocked)).unwrap();
        fs::write(root.join(&later), "later changed\n").unwrap();

        let errors = snapshot.rollback(root);

        assert!(!errors.is_empty());
        assert!(root.join(blocked).is_dir());
        assert_eq!(
            fs::read_to_string(root.join(later)).unwrap(),
            "later before\n"
        );
    }

    fn repository() -> TempDir {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/existing.md"), "committed\n").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "criv test"]);
        git(root, &["config", "user.email", "criv@example.invalid"]);
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "test: initialize transaction"]);
        temp
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_PREFIX")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
}

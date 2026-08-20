use std::fs;
use std::path::{Path, PathBuf};

use crate::git;
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

pub(crate) struct Snapshot {
    files: RepositoryFiles,
    index: git::IndexSnapshot,
    paths: Vec<CapturedPath>,
}

struct CapturedPath {
    path: String,
    contents: Option<String>,
    permissions: Option<fs::Permissions>,
}

impl Snapshot {
    #[cfg(test)]
    pub(crate) fn capture(root: &Path, paths: &[String]) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::capture_from(&files, paths)
    }

    pub(crate) fn capture_from(files: &RepositoryFiles, paths: &[String]) -> Result<Self> {
        Ok(Self {
            files: files.clone(),
            index: git::IndexSnapshot::capture(files)?,
            paths: paths
                .iter()
                .map(|path| CapturedPath::capture(files, path))
                .collect::<Result<Vec<_>>>()?,
        })
    }

    pub(crate) fn rollback(self, _root: &Path) -> Vec<String> {
        let mut errors = Vec::new();
        if let Err(error) = self.index.restore() {
            errors.push(error.to_string());
        }
        for path in self.paths {
            if let Err(error) = path.restore(&self.files) {
                errors.push(error.to_string());
            }
        }
        errors
    }
}

impl CapturedPath {
    fn capture(files: &RepositoryFiles, path: &str) -> Result<Self> {
        let relative = Path::new(path);
        if !files.file_exists(relative)? {
            return Ok(Self {
                path: path.to_string(),
                contents: None,
                permissions: None,
            });
        }
        let (contents, permissions) = files.read_with_permissions(relative)?;
        Ok(Self {
            path: path.to_string(),
            contents: Some(String::from_utf8(contents).map_err(|error| {
                CrivError::new(format!("snapshot file `{path}` is not UTF-8: {error}"))
            })?),
            permissions: Some(permissions),
        })
    }

    fn restore(self, files: &RepositoryFiles) -> Result<()> {
        let relative = PathBuf::from(&self.path);
        match (self.contents, self.permissions) {
            (Some(contents), Some(permissions)) => files
                .write_scope(Path::new("."))?
                .write_atomic_with_permissions(&relative, &contents, permissions),
            (None, None) if files.file_exists(&relative)? => {
                files.write_scope(Path::new("."))?.remove_file(&relative)
            }
            (None, None) => Ok(()),
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
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

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
        let index_before = git_output(root, &["ls-files", "--stage"]);

        let snapshot = Snapshot::capture(root, &[existing.clone(), receipt.clone()]).unwrap();
        fs::write(root.join(&existing), "changed\n").unwrap();
        #[cfg(unix)]
        set_mode(&root.join(&existing), 0o755);
        fs::write(root.join(&receipt), "new receipt\n").unwrap();
        git(root, &["add", "-A"]);

        assert!(snapshot.rollback(root).is_empty());
        assert_eq!(fs::read_to_string(root.join(existing)).unwrap(), "staged\n");
        assert!(!root.join(receipt).exists());
        assert_eq!(git_output(root, &["ls-files", "--stage"]), index_before);
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

    #[test]
    fn rollback_restores_index_only_flags_with_a_clean_worktree() {
        let temp = repository();
        let root = temp.path();
        fs::write(root.join("assume.txt"), "assume unchanged\n").unwrap();
        fs::write(root.join("skip.txt"), "skip worktree\n").unwrap();
        git(root, &["add", "assume.txt", "skip.txt"]);
        git(root, &["commit", "-qm", "test: add flagged files"]);
        git(root, &["update-index", "--assume-unchanged", "assume.txt"]);
        git(root, &["update-index", "--skip-worktree", "skip.txt"]);
        assert!(git_output(root, &["status", "--porcelain=v1"]).is_empty());
        let flags_before = git_output(root, &["ls-files", "-v", "--", "assume.txt", "skip.txt"]);
        assert!(flags_before.lines().any(|line| line == "h assume.txt"));
        assert!(flags_before.lines().any(|line| line == "S skip.txt"));
        let index_before = fs::read(root.join(".git/index")).unwrap();

        let snapshot = Snapshot::capture(root, &[]).unwrap();
        git(
            root,
            &["update-index", "--no-assume-unchanged", "assume.txt"],
        );
        git(root, &["update-index", "--no-skip-worktree", "skip.txt"]);
        assert_ne!(
            git_output(root, &["ls-files", "-v", "--", "assume.txt", "skip.txt"]),
            flags_before
        );

        assert!(snapshot.rollback(root).is_empty());
        assert_eq!(
            git_output(root, &["ls-files", "-v", "--", "assume.txt", "skip.txt"]),
            flags_before
        );
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
        assert!(git_output(root, &["status", "--porcelain=v1"]).is_empty());
        assert_eq!(
            fs::read_to_string(root.join("assume.txt")).unwrap(),
            "assume unchanged\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("skip.txt")).unwrap(),
            "skip worktree\n"
        );
    }

    #[test]
    fn rollback_restores_conflict_stages() {
        let temp = repository();
        let root = temp.path();
        let base = git_input(root, &["hash-object", "-w", "--stdin"], "base\n");
        let ours = git_input(root, &["hash-object", "-w", "--stdin"], "ours\n");
        let theirs = git_input(root, &["hash-object", "-w", "--stdin"], "theirs\n");
        let entries = format!(
            "100644 {base} 1\tconflict.txt\n100644 {ours} 2\tconflict.txt\n100644 {theirs} 3\tconflict.txt\n"
        );
        git_input(root, &["update-index", "--index-info"], &entries);
        let stages_before = git_output(root, &["ls-files", "--stage", "--", "conflict.txt"]);
        assert_eq!(stages_before.lines().count(), 3);
        assert!(
            stages_before
                .lines()
                .any(|line| line.contains(" 1\tconflict.txt"))
        );
        assert!(
            stages_before
                .lines()
                .any(|line| line.contains(" 2\tconflict.txt"))
        );
        assert!(
            stages_before
                .lines()
                .any(|line| line.contains(" 3\tconflict.txt"))
        );
        let index_before = fs::read(root.join(".git/index")).unwrap();

        let snapshot = Snapshot::capture(root, &[]).unwrap();
        git(root, &["reset", "--mixed", "HEAD"]);
        assert!(git_output(root, &["ls-files", "--stage", "--", "conflict.txt"]).is_empty());

        assert!(snapshot.rollback(root).is_empty());
        assert_eq!(
            git_output(root, &["ls-files", "--stage", "--", "conflict.txt"]),
            stages_before
        );
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
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
        let output = git_command(root).args(args).output().unwrap();
        assert_git_success(args, &output);
    }

    fn git_output(root: &Path, args: &[&str]) -> String {
        let output = git_command(root).args(args).output().unwrap();
        assert_git_success(args, &output);
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_string()
    }

    fn git_input(root: &Path, args: &[&str], input: &str) -> String {
        let mut child = git_command(root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_git_success(args, &output);
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_string()
    }

    fn git_command(root: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_PREFIX");
        command
    }

    fn assert_git_success(args: &[&str], output: &std::process::Output) {
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

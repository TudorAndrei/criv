//! Capability-rooted access to repository files.

mod filesystem;

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Result;

#[derive(Clone)]
pub(crate) struct RepositoryFiles {
    filesystem: Arc<filesystem::FileSystem>,
}

impl fmt::Debug for RepositoryFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryFiles")
            .field("root", &self.root())
            .finish_non_exhaustive()
    }
}

impl RepositoryFiles {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            filesystem: Arc::new(filesystem::FileSystem::open(root)?),
        })
    }

    pub(crate) fn root(&self) -> &Path {
        self.filesystem.root()
    }

    pub(crate) fn write_scope<'a>(
        &'a self,
        allowed_dir: &Path,
    ) -> Result<RepositoryWriteScope<'a>> {
        self.filesystem.validate_scope(allowed_dir)?;
        Ok(RepositoryWriteScope {
            files: self,
            allowed_dir: allowed_dir.to_path_buf(),
        })
    }

    pub(crate) fn read(&self, source: &Path) -> Result<Vec<u8>> {
        self.filesystem.read(source)
    }

    pub(crate) fn read_with_permissions(
        &self,
        source: &Path,
    ) -> Result<(Vec<u8>, fs::Permissions)> {
        self.filesystem.read_with_permissions(source)
    }

    pub(crate) fn read_optional_with_permissions(
        &self,
        source: &Path,
    ) -> Result<Option<(Vec<u8>, fs::Permissions)>> {
        self.filesystem.read_optional_with_permissions(source)
    }

    pub(crate) fn read_with_metadata(&self, source: &Path) -> Result<(Vec<u8>, fs::Metadata)> {
        self.filesystem.read_with_metadata(source)
    }

    pub(crate) fn read_bounded(&self, source: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        self.filesystem.read_bounded(source, max_bytes)
    }

    pub(crate) fn read_string(&self, source: &Path) -> Result<String> {
        self.filesystem.read_string(source)
    }

    pub(crate) fn read_optional_string(&self, source: &Path) -> Result<Option<String>> {
        self.filesystem.read_optional_string(source)
    }

    pub(crate) fn file_exists(&self, source: &Path) -> Result<bool> {
        self.filesystem.file_exists(source)
    }

    pub(crate) fn directory_exists(&self, source: &Path) -> Result<bool> {
        self.filesystem.directory_exists(source)
    }

    pub(crate) fn read_dir_names(&self, source: &Path) -> Result<Option<Vec<OsString>>> {
        self.filesystem.read_dir_names(source)
    }

    pub(crate) fn link_layout(&self, destination: &Path, target: &Path) -> Result<LinkLayout> {
        self.filesystem.link_layout(destination, target)
    }
}

pub(crate) struct RepositoryWriteScope<'a> {
    files: &'a RepositoryFiles,
    allowed_dir: PathBuf,
}

impl fmt::Debug for RepositoryWriteScope<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryWriteScope")
            .field("root", &self.files.root())
            .field("allowed_dir", &self.allowed_dir)
            .finish_non_exhaustive()
    }
}

impl RepositoryWriteScope<'_> {
    pub(crate) fn create_dir(&self, destination: &Path) -> Result<()> {
        self.files
            .filesystem
            .create_dir(&self.allowed_dir, destination)
    }

    pub(crate) fn write_new(&self, destination: &Path, contents: &str) -> Result<bool> {
        self.files
            .filesystem
            .write_new(&self.allowed_dir, destination, contents)
    }

    pub(crate) fn append_line_if_missing(&self, destination: &Path, line: &str) -> Result<()> {
        self.files
            .filesystem
            .append_line_if_missing(&self.allowed_dir, destination, line)
    }

    pub(crate) fn write_atomic(&self, destination: &Path, contents: &str) -> Result<()> {
        self.files
            .filesystem
            .write_atomic(&self.allowed_dir, destination, contents)
    }

    pub(crate) fn write_atomic_with_permissions(
        &self,
        destination: &Path,
        contents: &str,
        permissions: fs::Permissions,
    ) -> Result<()> {
        self.files.filesystem.write_atomic_with_permissions(
            &self.allowed_dir,
            destination,
            contents,
            permissions,
        )
    }

    pub(crate) fn write_atomic_bytes_with_permissions(
        &self,
        destination: &Path,
        contents: &[u8],
        permissions: fs::Permissions,
    ) -> Result<()> {
        self.files.filesystem.write_atomic_bytes_with_permissions(
            &self.allowed_dir,
            destination,
            contents,
            permissions,
        )
    }

    #[cfg(test)]
    pub(crate) fn write_atomic_if_changed(
        &self,
        destination: &Path,
        contents: &str,
    ) -> Result<bool> {
        self.files
            .filesystem
            .write_atomic_if_changed(&self.allowed_dir, destination, contents)
    }

    pub(crate) fn remove_file(&self, destination: &Path) -> Result<()> {
        self.files
            .filesystem
            .remove_file(&self.allowed_dir, destination)
    }

    pub(crate) fn rename_file(&self, source: &Path, destination: &Path) -> Result<()> {
        self.files
            .filesystem
            .rename_file(&self.allowed_dir, source, destination)
    }

    pub(crate) fn open_regular_file(&self, destination: &Path) -> Result<(PathBuf, fs::File)> {
        self.files
            .filesystem
            .open_regular_file(&self.allowed_dir, destination)
    }

    pub(crate) fn remove_empty_dir(&self, destination: &Path) -> Result<()> {
        self.files
            .filesystem
            .remove_empty_dir(&self.allowed_dir, destination)
    }

    pub(crate) fn link_dir(
        &self,
        destination: &Path,
        target: &Path,
        replace_directory: bool,
    ) -> Result<LinkOutcome> {
        self.files
            .filesystem
            .link_dir(&self.allowed_dir, destination, target, replace_directory)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum LinkOutcome {
    Unchanged,
    Created,
    Replaced,
    DirectoryInTheWay,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum LinkLayout {
    Missing,
    Expected,
    Directory,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_interface_reads_and_mutates_regular_files() {
        let root = tempfile::TempDir::new().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let scope = files.write_scope(Path::new("docs")).unwrap();
        let destination = Path::new("docs/generated/architecture.md");

        scope.create_dir(Path::new("docs/generated")).unwrap();
        assert!(scope.write_new(destination, "first\n").unwrap());
        assert!(!scope.write_new(destination, "ignored\n").unwrap());
        scope.append_line_if_missing(destination, "second").unwrap();
        scope.append_line_if_missing(destination, "second").unwrap();

        assert_eq!(files.read_string(destination).unwrap(), "first\nsecond\n");
        assert!(files.file_exists(destination).unwrap());
        assert!(files.directory_exists(Path::new("docs/generated")).unwrap());
        assert_eq!(
            files
                .read_dir_names(Path::new("docs/generated"))
                .unwrap()
                .unwrap(),
            vec![OsString::from("architecture.md")]
        );

        scope
            .rename_file(destination, Path::new("docs/generated/current.md"))
            .unwrap();
        scope
            .remove_file(Path::new("docs/generated/current.md"))
            .unwrap();
        scope.remove_empty_dir(Path::new("docs/generated")).unwrap();
        assert!(!files.directory_exists(Path::new("docs/generated")).unwrap());
    }

    #[test]
    fn write_scope_rejects_paths_outside_its_directory() {
        let root = tempfile::TempDir::new().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let scope = files.write_scope(Path::new("docs")).unwrap();

        assert!(scope.write_atomic(Path::new("state.json"), "{}\n").is_err());
        assert!(
            scope
                .rename_file(Path::new("docs/a.md"), Path::new("outside.md"))
                .is_err()
        );
        assert!(
            scope
                .rename_file(Path::new("outside.md"), Path::new("docs/a.md"))
                .is_err()
        );
    }

    #[test]
    fn parent_interface_rejects_invalid_and_non_regular_reads() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("directory")).unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();

        assert!(files.read(Path::new("")).is_err());
        assert!(files.read(Path::new("../secret")).is_err());
        assert!(files.read(Path::new("directory")).is_err());
        assert_eq!(
            files
                .read_optional_string(Path::new("missing.txt"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn atomic_write_replaces_contents_without_temporary_files() {
        let root = tempfile::TempDir::new().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let scope = files.write_scope(Path::new(".")).unwrap();
        let destination = Path::new("state.json");

        scope.write_atomic(destination, "{\"old\":true}\n").unwrap();
        scope.write_atomic(destination, "{\"new\":true}\n").unwrap();

        assert_eq!(files.read_string(destination).unwrap(), "{\"new\":true}\n");
        assert_eq!(
            files.read_dir_names(Path::new(".")).unwrap().unwrap(),
            vec![OsString::from("state.json")]
        );
    }

    #[test]
    fn parent_interface_round_trips_file_permissions() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("script"), "old\n").unwrap();
        let mut permissions = std::fs::metadata(root.path().join("script"))
            .unwrap()
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(root.path().join("script"), permissions).unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let (contents, permissions) = files.read_with_permissions(Path::new("script")).unwrap();

        assert_eq!(contents, b"old\n");
        files
            .write_scope(Path::new("."))
            .unwrap()
            .write_atomic_with_permissions(Path::new("script"), "new\n", permissions)
            .unwrap();

        assert_eq!(files.read_string(Path::new("script")).unwrap(), "new\n");
        assert!(
            std::fs::metadata(root.path().join("script"))
                .unwrap()
                .permissions()
                .readonly()
        );
        let mut cleanup_permissions = std::fs::metadata(root.path().join("script"))
            .unwrap()
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            cleanup_permissions.set_mode(cleanup_permissions.mode() | 0o200);
        }
        #[cfg(windows)]
        cleanup_permissions.set_readonly(false);
        std::fs::set_permissions(root.path().join("script"), cleanup_permissions).unwrap();
    }

    #[test]
    fn parent_interface_owns_generated_directory_links() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let scope = files.write_scope(Path::new(".")).unwrap();
        let destination = Path::new(".claude/skills");
        let target = Path::new(".agents/skills");

        let outcome = scope.link_dir(destination, target, false).unwrap();
        if outcome == LinkOutcome::Unsupported {
            assert_eq!(
                files.link_layout(destination, target).unwrap(),
                LinkLayout::Missing
            );
        } else {
            assert_eq!(outcome, LinkOutcome::Created);
            assert_eq!(
                files.link_layout(destination, target).unwrap(),
                LinkLayout::Expected
            );
            assert_eq!(
                scope.link_dir(destination, target, false).unwrap(),
                LinkOutcome::Unchanged
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn interface_rejects_linked_components_and_final_links() {
        use std::os::unix::fs::symlink;

        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        symlink(outside.path(), root.path().join("docs")).unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();

        assert!(files.read(Path::new("docs/file.md")).is_err());
        assert!(
            files
                .write_scope(Path::new("docs"))
                .unwrap()
                .write_atomic(Path::new("docs/file.md"), "changed\n")
                .is_err()
        );

        std::fs::remove_file(root.path().join("docs")).unwrap();
        std::fs::write(outside.path().join("target.md"), "outside\n").unwrap();
        symlink(
            outside.path().join("target.md"),
            root.path().join("target.md"),
        )
        .unwrap();
        let scope = files.write_scope(Path::new(".")).unwrap();
        assert!(
            scope
                .write_atomic(Path::new("target.md"), "changed\n")
                .is_err()
        );
        assert!(scope.remove_file(Path::new("target.md")).is_err());
        assert_eq!(
            std::fs::read_to_string(outside.path().join("target.md")).unwrap(),
            "outside\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn held_root_cannot_be_redirected_after_open() {
        use std::os::unix::fs::symlink;

        let container = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        let root = container.path().join("repository");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(outside.path().join("file.md"), "outside\n").unwrap();
        let files = RepositoryFiles::open(&root).unwrap();

        std::fs::rename(&root, container.path().join("held-repository")).unwrap();
        symlink(outside.path(), &root).unwrap();

        let result = files
            .write_scope(Path::new("docs"))
            .unwrap()
            .write_atomic(Path::new("docs/file.md"), "inside\n");

        result.unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.path().join("file.md")).unwrap(),
            "outside\n"
        );
        assert_eq!(
            files.read_string(Path::new("docs/file.md")).unwrap(),
            "inside\n"
        );
    }
}

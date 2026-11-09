use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(windows))]
use cap_fs_ext::OpenOptionsMaybeDirExt;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions, Permissions as CapPermissions};

use super::{EntryKind, LinkLayout, LinkOutcome};
use crate::{CrivError, Result};

/// One repository root, held as a capability directory handle.
#[derive(Clone)]
pub struct RepositoryFiles {
    root_path: PathBuf,
    root: Arc<Dir>,
}

impl fmt::Debug for RepositoryFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryFiles")
            .field("root", &self.root_path)
            .finish_non_exhaustive()
    }
}

/// Mutations confined to one allowed directory of a [`RepositoryFiles`] root.
pub struct RepositoryWriteScope<'a> {
    files: &'a RepositoryFiles,
    allowed_dir: PathBuf,
}

impl fmt::Debug for RepositoryWriteScope<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryWriteScope")
            .field("root", &self.files.root_path)
            .field("allowed_dir", &self.allowed_dir)
            .finish_non_exhaustive()
    }
}

impl RepositoryFiles {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        let root_path = fs::canonicalize(root).map_err(|error| {
            CrivError::new(format!(
                "failed to resolve vault root {} for confined access: {error}",
                root.display()
            ))
        })?;
        let root = Dir::open_ambient_dir(&root_path, ambient_authority())?;
        Ok(Self {
            root_path,
            root: Arc::new(root),
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root_path
    }

    pub(crate) fn write_scope(&self, allowed_dir: &Path) -> Result<RepositoryWriteScope<'_>> {
        validate_relative_path("allowed write directory", allowed_dir)?;
        Ok(RepositoryWriteScope {
            files: self,
            allowed_dir: allowed_dir.to_path_buf(),
        })
    }

    pub(crate) fn read(&self, source: &Path) -> Result<Vec<u8>> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(contents)
    }

    pub(crate) fn permissions(&self, source: &Path) -> Result<fs::Permissions> {
        let file = self.for_read(source)?.open_required_regular()?;
        let permissions = file.metadata()?.permissions();
        let file = file.into_std();
        Ok(permissions.into_std(&file)?)
    }

    pub(crate) fn read_optional_with_permissions(
        &self,
        source: &Path,
    ) -> Result<Option<(Vec<u8>, fs::Permissions)>> {
        let Some(target) = self.for_read_optional(source)? else {
            return Ok(None);
        };
        let Some(mut file) = target.open_regular()? else {
            return Ok(None);
        };
        let permissions = file.metadata()?.permissions();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        let file = file.into_std();
        Ok(Some((contents, permissions.into_std(&file)?)))
    }

    pub(crate) fn read_with_metadata(&self, source: &Path) -> Result<(Vec<u8>, fs::Metadata)> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        let file = file.into_std();
        Ok((contents, file.metadata()?))
    }

    pub(crate) fn read_bounded(&self, source: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let size = file.metadata()?.len();
        if size > max_bytes {
            return Ok(None);
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        if u64::try_from(contents.len()).map_or(true, |length| length > max_bytes) {
            return Ok(None);
        }
        Ok(Some(contents))
    }

    pub(crate) fn read_string(&self, source: &Path) -> Result<String> {
        self.for_read(source)?
            .read_optional_string()?
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).into())
    }

    pub(crate) fn read_optional_string(&self, source: &Path) -> Result<Option<String>> {
        let Some(target) = self.for_read_optional(source)? else {
            return Ok(None);
        };
        target.read_optional_string()
    }

    pub(crate) fn file_exists(&self, source: &Path) -> Result<bool> {
        let Some(target) = self.for_read_optional(source)? else {
            return Ok(false);
        };
        Ok(target.open_regular()?.is_some())
    }

    pub(crate) fn directory_exists(&self, source: &Path) -> Result<bool> {
        validate_relative_path("directory source", source)?;
        Ok(self.open_dir_optional(source, false)?.is_some())
    }

    pub(crate) fn entry_exists(&self, source: &Path) -> Result<bool> {
        validate_relative_path("entry source", source)?;
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let Some(directory) = self.open_dir_optional(parent, false)? else {
            return Ok(false);
        };
        let name = source.file_name().ok_or_else(|| {
            CrivError::new(format!(
                "repository entry path {} has no name",
                source.display()
            ))
        })?;
        match directory.symlink_metadata(name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn entry_kind(&self, source: &Path) -> EntryKind {
        if source.as_os_str().is_empty() {
            return EntryKind::Directory;
        }
        let Some(name) = source.file_name() else {
            return EntryKind::Other;
        };
        if validate_relative_path("entry source", source).is_err() {
            return EntryKind::Other;
        }
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let directory = match self.open_dir_optional(parent, false) {
            Ok(Some(directory)) => directory,
            Ok(None) => return EntryKind::Missing,
            Err(_) => return EntryKind::Other,
        };
        match directory.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() => EntryKind::Other,
            Ok(metadata) if metadata.is_file() => EntryKind::File,
            Ok(metadata) if metadata.is_dir() => EntryKind::Directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => EntryKind::Missing,
            Ok(_) | Err(_) => EntryKind::Other,
        }
    }

    pub(crate) fn read_dir_names(&self, source: &Path) -> Result<Option<Vec<OsString>>> {
        validate_relative_path("directory source", source)?;
        let Some(directory) = self.open_dir_optional(source, false)? else {
            return Ok(None);
        };
        let mut names = Vec::new();
        for entry in directory.entries()? {
            names.push(entry?.file_name());
        }
        Ok(Some(names))
    }

    pub(crate) fn link_layout(&self, destination: &Path, target: &Path) -> Result<LinkLayout> {
        validate_relative_path("link destination", destination)?;
        validate_relative_path("link target", target)?;
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let Some(directory) = self.open_dir_optional(parent, false)? else {
            return Ok(LinkLayout::Missing);
        };
        let name = destination.file_name().ok_or_else(|| {
            CrivError::new(format!(
                "repository link path {} has no name",
                destination.display()
            ))
        })?;
        match directory.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let current = directory.read_link_contents(name)?;
                Ok(
                    if current == relative_link_target(destination, target)
                        || self.resolves_to(destination, target)
                    {
                        LinkLayout::Expected
                    } else {
                        LinkLayout::Other
                    },
                )
            }
            Ok(metadata) if metadata.is_dir() => Ok(if self.resolves_to(destination, target) {
                LinkLayout::Expected
            } else {
                LinkLayout::Directory
            }),
            Ok(_) => Ok(LinkLayout::Other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LinkLayout::Missing),
            Err(error) => Err(error.into()),
        }
    }

    /// Whether `destination` and `target` name the same physical directory.
    /// A dangling or unreadable path names none.
    fn resolves_to(&self, destination: &Path, target: &Path) -> bool {
        fs::canonicalize(self.root_path.join(destination))
            .ok()
            .zip(fs::canonicalize(self.root_path.join(target)).ok())
            .is_some_and(|(link, target)| link == target)
    }

    fn for_read(&self, source: &Path) -> Result<ConfinedFile> {
        self.for_read_optional(source)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault file {} does not exist", source.display()),
            )
            .into()
        })
    }

    fn for_read_optional(&self, source: &Path) -> Result<Option<ConfinedFile>> {
        validate_relative_path("read source", source)?;
        self.prepare_optional(source, false)
    }

    fn for_write(&self, allowed_dir: &Path, destination: &Path) -> Result<ConfinedFile> {
        self.for_write_optional(allowed_dir, destination, true)?
            .ok_or_else(|| {
                CrivError::new(format!(
                    "failed to create parent for {}",
                    destination.display()
                ))
            })
    }

    fn for_write_optional(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        create_parents: bool,
    ) -> Result<Option<ConfinedFile>> {
        validate_destination(allowed_dir, destination, "write destination")?;
        self.prepare_optional(destination, create_parents)
    }

    fn prepare_optional(
        &self,
        relative: &Path,
        create_parents: bool,
    ) -> Result<Option<ConfinedFile>> {
        let parent = relative.parent().unwrap_or_else(|| Path::new("."));
        let Some(directory) = self.open_dir_optional(parent, create_parents)? else {
            return Ok(None);
        };
        let name = relative.file_name().ok_or_else(|| {
            CrivError::new(format!(
                "repository file path {} has no name",
                relative.display()
            ))
        })?;
        Ok(Some(ConfinedFile {
            parent: directory,
            name: name.to_os_string(),
            relative: relative.to_path_buf(),
        }))
    }

    fn open_dir(&self, relative: &Path, create: bool) -> Result<Dir> {
        self.open_dir_optional(relative, create)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault directory {} does not exist", relative.display()),
            )
            .into()
        })
    }

    /// Opens every component with a no-follow, handle-relative open, which is
    /// the confinement authority. A refused component is described from the
    /// same handle.
    fn open_dir_optional(&self, relative: &Path, create: bool) -> Result<Option<Dir>> {
        let mut directory = self.root.try_clone()?;
        let mut opened = PathBuf::new();
        for component in relative.components() {
            let std::path::Component::Normal(part) = component else {
                continue;
            };
            opened.push(part);
            directory = match directory.open_dir_nofollow(part) {
                Ok(next) => next,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Ok(None);
                    }
                    match directory.create_dir(part) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
                    }
                    directory.open_dir_nofollow(part)?
                }
                Err(error) => return Err(refused_component(&directory, part, &opened, error)),
            };
        }
        Ok(Some(directory))
    }
}

fn refused_component(
    parent: &Dir,
    part: &std::ffi::OsStr,
    path: &Path,
    error: std::io::Error,
) -> CrivError {
    match parent.symlink_metadata(part) {
        Ok(metadata) if metadata.file_type().is_symlink() => CrivError::new(format!(
            "refusing to write through symlinked vault path component {}",
            path.display()
        )),
        Ok(metadata) if !metadata.is_dir() => CrivError::new(format!(
            "cannot create vault path below non-directory component {}",
            path.display()
        )),
        _ => error.into(),
    }
}

impl RepositoryWriteScope<'_> {
    pub(crate) fn create_dir(&self, destination: &Path) -> Result<()> {
        validate_destination(&self.allowed_dir, destination, "directory destination")?;
        self.files.open_dir(destination, true)?;
        Ok(())
    }

    pub(crate) fn write_new(&self, destination: &Path, contents: &str) -> Result<bool> {
        let target = self.target(destination)?;
        if target.open_regular()?.is_some() {
            return Ok(false);
        }
        target.write_atomic(contents.as_bytes(), None)?;
        Ok(true)
    }

    pub(crate) fn append_line_if_missing(&self, destination: &Path, line: &str) -> Result<()> {
        let target = self.target(destination)?;
        let mut contents = target.read_optional_string()?.unwrap_or_default();
        if !contents.lines().any(|existing| existing.trim() == line) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(line);
            contents.push('\n');
            target.write_atomic(contents.as_bytes(), None)?;
        }
        Ok(())
    }

    pub(crate) fn write_atomic(&self, destination: &Path, contents: &str) -> Result<()> {
        self.target(destination)?
            .write_atomic(contents.as_bytes(), None)
    }

    pub(crate) fn write_atomic_with_permissions(
        &self,
        destination: &Path,
        contents: &[u8],
        permissions: fs::Permissions,
    ) -> Result<()> {
        self.target(destination)?
            .write_atomic(contents, Some(CapPermissions::from_std(permissions)))
    }

    /// Creates a new regular file, failing when any entry already exists.
    pub(crate) fn create_new(&self, destination: &Path) -> Result<fs::File> {
        let target = self.target(destination)?;
        let mut options = nofollow_options();
        options.write(true).create_new(true);
        let file = target
            .parent
            .open_with(&target.name, &options)
            .map_err(|error| {
                CrivError::new(format!("cannot create {}: {error}", destination.display()))
            })?;
        Ok(file.into_std())
    }

    pub(crate) fn remove_file(&self, destination: &Path) -> Result<()> {
        self.target(destination)?.remove()
    }

    pub(crate) fn rename_file(&self, source: &Path, destination: &Path) -> Result<()> {
        let source = self.target(source)?;
        let destination = self.target(destination)?;
        source.rename_to(&destination)
    }

    /// Opens or creates a persistent lock file. A capability open that
    /// creates the file can report `NotFound` while another process creates
    /// the same file, so the open is retried a bounded number of times.
    pub(crate) fn open_regular_file(&self, destination: &Path) -> Result<(PathBuf, fs::File)> {
        const CREATE_RACE_RETRIES: usize = 9;
        let target = self.target(destination)?;
        let mut options = nofollow_options();
        options.read(true).write(true).create(true);
        let mut retries = 0_usize;
        let file = loop {
            match target.parent.open_with(&target.name, &options) {
                Ok(file) => break file,
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        && retries < CREATE_RACE_RETRIES =>
                {
                    retries = retries.saturating_add(1);
                }
                Err(error) => return Err(error.into()),
            }
        };
        if !file.metadata()?.is_file() {
            return Err(CrivError::new(format!(
                "vault path {} must be a regular file",
                destination.display()
            )));
        }
        Ok((self.files.root_path.join(destination), file.into_std()))
    }

    pub(crate) fn remove_empty_dir(&self, destination: &Path) -> Result<()> {
        let Some(target) = self
            .files
            .for_write_optional(&self.allowed_dir, destination, false)?
        else {
            return Ok(());
        };
        match target.parent.open_dir_nofollow(&target.name) {
            Ok(directory) => drop(directory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        match target.parent.remove_dir(&target.name) {
            Ok(()) => sync_directory_handle(&target.parent),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn link_dir(
        &self,
        destination: &Path,
        target: &Path,
        replace_directory: bool,
    ) -> Result<LinkOutcome> {
        validate_destination(&self.allowed_dir, destination, "link destination")?;
        validate_relative_path("link target", target)?;
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let link = ConfinedFile {
            parent: self.files.open_dir(parent, true)?,
            name: destination
                .file_name()
                .ok_or_else(|| {
                    CrivError::new(format!(
                        "repository link path {} has no name",
                        destination.display()
                    ))
                })?
                .to_os_string(),
            relative: destination.to_path_buf(),
        };
        let relative_target = relative_link_target(destination, target);
        // A Windows junction reads as a directory, so a destination that
        // already resolves to `target` is never replaced: removing it would
        // remove the target's contents.
        if self.files.resolves_to(destination, target) {
            return Ok(LinkOutcome::Unchanged);
        }
        let created = match link.parent.symlink_metadata(&link.name) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if link.parent.read_link_contents(&link.name)? == relative_target {
                    return Ok(LinkOutcome::Unchanged);
                }
                self.remove_link(&link)?;
                LinkOutcome::Created
            }
            Ok(metadata) if metadata.is_dir() => {
                if !replace_directory {
                    return Ok(LinkOutcome::DirectoryInTheWay);
                }
                link.parent.remove_dir_all(&link.name)?;
                sync_directory_handle(&link.parent)?;
                LinkOutcome::Replaced
            }
            Ok(_) => {
                link.parent.remove_file(&link.name)?;
                sync_directory_handle(&link.parent)?;
                LinkOutcome::Created
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => LinkOutcome::Created,
            Err(error) => return Err(error.into()),
        };
        match self.create_link(&link, &relative_target, target) {
            Ok(()) => {
                sync_directory_handle(&link.parent)?;
                Ok(created)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Unsupported | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                Ok(LinkOutcome::Unsupported)
            }
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(unix)]
    #[expect(
        clippy::unused_self,
        reason = "the Windows junction is created from the repository root"
    )]
    fn create_link(
        &self,
        link: &ConfinedFile,
        relative_target: &Path,
        _target: &Path,
    ) -> std::io::Result<()> {
        link.parent.symlink_contents(relative_target, &link.name)
    }

    #[cfg(windows)]
    fn create_link(
        &self,
        link: &ConfinedFile,
        _relative_target: &Path,
        target: &Path,
    ) -> std::io::Result<()> {
        junction::create(
            self.files.root_path.join(target),
            self.files.root_path.join(&link.relative),
        )
    }

    #[cfg(not(any(unix, windows)))]
    #[expect(clippy::unused_self, reason = "no platform link primitive exists")]
    fn create_link(
        &self,
        _link: &ConfinedFile,
        _relative_target: &Path,
        _target: &Path,
    ) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    #[cfg(not(windows))]
    #[expect(
        clippy::unused_self,
        reason = "the Windows junction is removed from the repository root"
    )]
    fn remove_link(&self, link: &ConfinedFile) -> Result<()> {
        link.parent.remove_file(&link.name)?;
        sync_directory_handle(&link.parent)
    }

    #[cfg(windows)]
    fn remove_link(&self, link: &ConfinedFile) -> Result<()> {
        let path = self.files.root_path.join(&link.relative);
        if junction::exists(&path)? {
            junction::delete(&path)?;
        }
        match link.parent.remove_dir(&link.name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        sync_directory_handle(&link.parent)
    }

    fn target(&self, destination: &Path) -> Result<ConfinedFile> {
        self.files.for_write(&self.allowed_dir, destination)
    }
}

struct ConfinedFile {
    parent: Dir,
    name: OsString,
    relative: PathBuf,
}

impl ConfinedFile {
    fn open_regular(&self) -> Result<Option<cap_std::fs::File>> {
        let mut options = nofollow_options();
        options.read(true);
        match self.parent.open_with(&self.name, &options) {
            Ok(file) if file.metadata()?.is_file() => Ok(Some(file)),
            Ok(_) => Err(CrivError::new(format!(
                "vault path {} must be a regular file",
                self.relative.display()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => match self.parent.symlink_metadata(&self.name) {
                Ok(metadata) if metadata.file_type().is_symlink() => Err(CrivError::new(format!(
                    "refusing to write through symlinked vault path component {}",
                    self.relative.display()
                ))),
                _ => Err(error.into()),
            },
        }
    }

    fn open_required_regular(&self) -> Result<cap_std::fs::File> {
        self.open_regular()?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault file {} does not exist", self.relative.display()),
            )
            .into()
        })
    }

    fn read_optional_string(&self) -> Result<Option<String>> {
        let Some(mut file) = self.open_regular()? else {
            return Ok(None);
        };
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(Some(contents))
    }

    fn write_atomic(&self, contents: &[u8], permissions: Option<CapPermissions>) -> Result<()> {
        let inherited_permissions = match permissions {
            Some(permissions) => Some(permissions),
            None => self
                .open_regular()?
                .map(|file| file.metadata().map(|metadata| metadata.permissions()))
                .transpose()?,
        };
        let file_name = self.name.to_string_lossy();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        for attempt in 0_u32..100 {
            let temp_name = OsString::from(format!(
                ".{file_name}.{}.{}.tmp",
                std::process::id(),
                nonce.saturating_add(u128::from(attempt))
            ));
            let mut options = nofollow_options();
            options.write(true).create_new(true);
            let mut file = match self.parent.open_with(&temp_name, &options) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            let result = (|| -> Result<()> {
                file.write_all(contents)?;
                file.sync_all()?;
                if let Some(permissions) = inherited_permissions.clone() {
                    file.set_permissions(permissions)?;
                }
                drop(file);
                #[cfg(windows)]
                let replaced_permissions = self.prepare_destination_for_replace()?;
                #[cfg(not(windows))]
                let replaced_permissions = Self::prepare_destination_for_replace();
                if let Err(error) = self.parent.rename(&temp_name, &self.parent, &self.name) {
                    if let Some(permissions) = replaced_permissions {
                        self.parent.set_permissions(&self.name, permissions)?;
                    }
                    return Err(error.into());
                }
                Ok(())
            })();
            if let Err(error) = result {
                return Err(match self.parent.remove_file(&temp_name) {
                    Ok(()) => error,
                    Err(cleanup) => CrivError::new(format!(
                        "{error}; removing temporary file {} also failed: {cleanup}",
                        self.relative.with_file_name(&temp_name).display()
                    )),
                });
            }
            return sync_directory_handle(&self.parent);
        }

        Err(CrivError::new(format!(
            "failed to create temporary file for {}",
            self.relative.display()
        )))
    }

    #[cfg(windows)]
    fn prepare_destination_for_replace(&self) -> Result<Option<CapPermissions>> {
        let metadata = match self.parent.symlink_metadata(&self.name) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let original = metadata.permissions();
        if !original.readonly() {
            return Ok(None);
        }
        let mut writable = original.clone();
        writable.set_readonly(false);
        self.parent.set_permissions(&self.name, writable)?;
        Ok(Some(original))
    }

    #[cfg(not(windows))]
    const fn prepare_destination_for_replace() -> Option<CapPermissions> {
        None
    }

    fn remove(&self) -> Result<()> {
        self.open_required_regular()?;
        self.parent.remove_file(&self.name)?;
        sync_directory_handle(&self.parent)
    }

    fn rename_to(&self, destination: &Self) -> Result<()> {
        self.open_required_regular()?;
        self.parent
            .rename(&self.name, &destination.parent, &destination.name)?;
        sync_directory_handle(&self.parent)?;
        sync_directory_handle(&destination.parent)
    }
}

fn nofollow_options() -> CapOpenOptions {
    let mut options = CapOpenOptions::new();
    options.follow(FollowSymlinks::No);
    options
}

#[cfg(windows)]
fn sync_directory_handle(_directory: &Dir) -> Result<()> {
    Ok(())
}

#[cfg(not(windows))]
fn sync_directory_handle(directory: &Dir) -> Result<()> {
    let mut options = nofollow_options();
    options.read(true).maybe_dir(true);
    directory.open_with(".", &options)?.sync_all()?;
    Ok(())
}

fn validate_destination(allowed_dir: &Path, destination: &Path, label: &str) -> Result<()> {
    validate_relative_path("allowed write directory", allowed_dir)?;
    validate_relative_path(label, destination)?;
    if allowed_dir != Path::new(".") && !destination.starts_with(allowed_dir) {
        return Err(CrivError::new(format!(
            "refusing to write {} outside allowed vault directory {}",
            destination.display(),
            allowed_dir.display()
        )));
    }
    Ok(())
}

fn validate_relative_path(label: &str, path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(CrivError::new(format!(
            "{label} must be a non-empty relative path"
        )));
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(CrivError::new(format!(
            "{label} must not contain parent-directory segments"
        )));
    }
    Ok(())
}

fn relative_link_target(destination: &Path, target: &Path) -> PathBuf {
    let depth = destination
        .parent()
        .map_or(0, |parent| parent.components().count());
    let mut relative = PathBuf::new();
    for _ in 0..depth {
        relative.push("..");
    }
    relative.push(target);
    relative
}

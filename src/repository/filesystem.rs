use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions, Permissions as CapPermissions};

use super::{LinkLayout, LinkOutcome};
use crate::{CrivError, Result};

pub(super) struct FileSystem {
    root_path: PathBuf,
    root: Dir,
}

impl FileSystem {
    pub(super) fn open(root: &Path) -> Result<Self> {
        let root_path = fs::canonicalize(root).map_err(|error| {
            CrivError::new(format!(
                "failed to resolve vault root {} for confined access: {error}",
                root.display()
            ))
        })?;
        let root = Dir::open_ambient_dir(&root_path, ambient_authority())?;
        Ok(Self { root_path, root })
    }

    pub(super) fn root(&self) -> &Path {
        &self.root_path
    }

    pub(super) fn validate_scope(&self, allowed_dir: &Path) -> Result<()> {
        validate_relative_path("allowed write directory", allowed_dir)
    }

    pub(super) fn create_dir(&self, allowed_dir: &Path, destination: &Path) -> Result<()> {
        self.validate_destination(allowed_dir, destination, "directory destination")?;
        self.open_dir(destination, true)?;
        Ok(())
    }

    pub(super) fn write_new(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        contents: &str,
    ) -> Result<bool> {
        let target = self.for_write(allowed_dir, destination)?;
        if target.open_regular(false)?.is_some() {
            return Ok(false);
        }
        target.write_atomic(contents, None)?;
        Ok(true)
    }

    pub(super) fn append_line_if_missing(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        line: &str,
    ) -> Result<()> {
        let target = self.for_write(allowed_dir, destination)?;
        let mut contents = target.read_optional_string()?.unwrap_or_default();
        if !contents.lines().any(|existing| existing.trim() == line) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(line);
            contents.push('\n');
            target.write_atomic(&contents, None)?;
        }
        Ok(())
    }

    pub(super) fn write_atomic(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        contents: &str,
    ) -> Result<()> {
        self.for_write(allowed_dir, destination)?
            .write_atomic(contents, None)
    }

    pub(super) fn write_atomic_with_permissions(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        contents: &str,
        permissions: fs::Permissions,
    ) -> Result<()> {
        self.for_write(allowed_dir, destination)?
            .write_atomic(contents, Some(CapPermissions::from_std(permissions)))
    }

    pub(super) fn write_atomic_bytes_with_permissions(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        contents: &[u8],
        permissions: fs::Permissions,
    ) -> Result<()> {
        self.for_write(allowed_dir, destination)?
            .write_atomic_bytes(contents, Some(CapPermissions::from_std(permissions)))
    }

    #[cfg(test)]
    pub(super) fn write_atomic_if_changed(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        contents: &str,
    ) -> Result<bool> {
        let target = self.for_write(allowed_dir, destination)?;
        if target.read_optional_string()?.as_deref() == Some(contents) {
            return Ok(false);
        }
        target.write_atomic(contents, None)?;
        Ok(true)
    }

    pub(super) fn remove_file(&self, allowed_dir: &Path, destination: &Path) -> Result<()> {
        self.for_write(allowed_dir, destination)?.remove()
    }

    pub(super) fn rename_file(
        &self,
        allowed_dir: &Path,
        source: &Path,
        destination: &Path,
    ) -> Result<()> {
        let source = self.for_write(allowed_dir, source)?;
        let destination = self.for_write(allowed_dir, destination)?;
        source.rename_to(&destination)
    }

    pub(super) fn open_regular_file(
        &self,
        allowed_dir: &Path,
        destination: &Path,
    ) -> Result<(PathBuf, fs::File)> {
        let target = self.for_write(allowed_dir, destination)?;
        let mut options = nofollow_options();
        options.read(true).write(true).create(true);
        let mut attempts = 0;
        let file = loop {
            match target.parent.open_with(&target.name, &options) {
                Ok(file) => break file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && attempts < 9 => {
                    attempts += 1;
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
        Ok((self.root_path.join(destination), file.into_std()))
    }

    pub(super) fn read(&self, source: &Path) -> Result<Vec<u8>> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(contents)
    }

    pub(super) fn read_with_permissions(
        &self,
        source: &Path,
    ) -> Result<(Vec<u8>, fs::Permissions)> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let permissions = file.metadata()?.permissions();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        let file = file.into_std();
        Ok((contents, permissions.into_std(&file)?))
    }

    pub(super) fn read_optional_with_permissions(
        &self,
        source: &Path,
    ) -> Result<Option<(Vec<u8>, fs::Permissions)>> {
        let Some(target) = self.for_read_optional(source)? else {
            return Ok(None);
        };
        let Some(mut file) = target.open_regular(false)? else {
            return Ok(None);
        };
        let permissions = file.metadata()?.permissions();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        let file = file.into_std();
        Ok(Some((contents, permissions.into_std(&file)?)))
    }

    pub(super) fn read_with_metadata(&self, source: &Path) -> Result<(Vec<u8>, fs::Metadata)> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        let file = file.into_std();
        Ok((contents, file.metadata()?))
    }

    pub(super) fn read_bounded(&self, source: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        let mut file = self.for_read(source)?.open_required_regular()?;
        let size = file.metadata()?.len();
        if size > max_bytes {
            return Ok(None);
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        if contents.len() as u64 > max_bytes {
            return Ok(None);
        }
        Ok(Some(contents))
    }

    pub(super) fn read_string(&self, source: &Path) -> Result<String> {
        self.for_read(source)?
            .read_optional_string()?
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).into())
    }

    pub(super) fn read_optional_string(&self, source: &Path) -> Result<Option<String>> {
        let Some(target) = self.for_read_optional(source)? else {
            return Ok(None);
        };
        target.read_optional_string()
    }

    pub(super) fn file_exists(&self, source: &Path) -> Result<bool> {
        let Some(target) = self.for_read_optional(source)? else {
            return Ok(false);
        };
        Ok(target.open_regular(false)?.is_some())
    }

    pub(super) fn directory_exists(&self, source: &Path) -> Result<bool> {
        validate_relative_path("directory source", source)?;
        Ok(self.open_dir_optional(source, false)?.is_some())
    }

    pub(super) fn read_dir_names(&self, source: &Path) -> Result<Option<Vec<OsString>>> {
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

    pub(super) fn remove_empty_dir(&self, allowed_dir: &Path, destination: &Path) -> Result<()> {
        let target = self.for_write_optional(allowed_dir, destination, false)?;
        let Some(target) = target else {
            return Ok(());
        };
        match target.parent.open_dir_nofollow(&target.name) {
            Ok(directory) => drop(directory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        match target.parent.remove_dir(&target.name) {
            Ok(()) => {
                sync_directory_handle(&target.parent)?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn link_dir(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        target: &Path,
        replace_directory: bool,
    ) -> Result<LinkOutcome> {
        self.validate_destination(allowed_dir, destination, "link destination")?;
        validate_relative_path("link target", target)?;
        let link = self.for_link(allowed_dir, destination)?;
        let relative_target = relative_link_target(destination, target);
        let target_path = self.root_path.join(target);

        match link.parent.symlink_metadata(&link.name) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let current = link
                    .parent
                    .read_link_contents(&link.name)
                    .unwrap_or_default();
                let resolves_to_target = fs::canonicalize(self.root_path.join(destination))
                    .ok()
                    .zip(fs::canonicalize(&target_path).ok())
                    .is_some_and(|(link, target)| link == target);
                if current == relative_target || resolves_to_target {
                    return Ok(LinkOutcome::Unchanged);
                }
                self.remove_link(&link)?;
            }
            Ok(metadata) if metadata.is_dir() => {
                if !replace_directory {
                    return Ok(LinkOutcome::DirectoryInTheWay);
                }
                link.parent.remove_dir_all(&link.name)?;
                sync_directory_handle(&link.parent)?;
                return self
                    .finish_link(&link, &relative_target, &target_path)
                    .map(|created| {
                        if created {
                            LinkOutcome::Replaced
                        } else {
                            LinkOutcome::Unsupported
                        }
                    });
            }
            Ok(_) => {
                link.parent.remove_file(&link.name)?;
                sync_directory_handle(&link.parent)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        self.finish_link(&link, &relative_target, &target_path)
            .map(|created| {
                if created {
                    LinkOutcome::Created
                } else {
                    LinkOutcome::Unsupported
                }
            })
    }

    pub(super) fn link_layout(&self, destination: &Path, target: &Path) -> Result<LinkLayout> {
        validate_relative_path("link destination", destination)?;
        validate_relative_path("link target", target)?;
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        reject_symlink_components(&self.root_path, parent)?;
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
                let relative_target = relative_link_target(destination, target);
                let current = directory.read_link_contents(name).unwrap_or_default();
                let resolves_to_target = fs::canonicalize(self.root_path.join(destination))
                    .ok()
                    .zip(fs::canonicalize(self.root_path.join(target)).ok())
                    .is_some_and(|(link, target)| link == target);
                Ok(if current == relative_target || resolves_to_target {
                    LinkLayout::Expected
                } else {
                    LinkLayout::Other
                })
            }
            Ok(metadata) if metadata.is_dir() => {
                let resolves_to_target = fs::canonicalize(self.root_path.join(destination))
                    .ok()
                    .zip(fs::canonicalize(self.root_path.join(target)).ok())
                    .is_some_and(|(link, target)| link == target);
                Ok(if resolves_to_target {
                    LinkLayout::Expected
                } else {
                    LinkLayout::Directory
                })
            }
            Ok(_) => Ok(LinkLayout::Other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LinkLayout::Missing),
            Err(error) => Err(error.into()),
        }
    }

    fn finish_link(
        &self,
        link: &ConfinedFile,
        relative_target: &Path,
        absolute_target: &Path,
    ) -> Result<bool> {
        #[cfg(unix)]
        let result = link.parent.symlink_contents(relative_target, &link.name);
        #[cfg(windows)]
        let result = junction::create(absolute_target, self.root_path.join(&link.relative));
        #[cfg(not(any(unix, windows)))]
        let result = Err(std::io::Error::other(
            "links are unsupported on this platform",
        ));

        #[cfg(not(windows))]
        let _ = absolute_target;
        #[cfg(not(unix))]
        let _ = relative_target;

        match result {
            Ok(()) => {
                sync_directory_handle(&link.parent)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn remove_link(&self, link: &ConfinedFile) -> Result<()> {
        #[cfg(windows)]
        {
            let path = self.root_path.join(&link.relative);
            if junction::exists(&path).unwrap_or(false) {
                junction::delete(&path)?;
            }
            match link.parent.remove_dir(&link.name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        #[cfg(not(windows))]
        link.parent.remove_file(&link.name)?;
        sync_directory_handle(&link.parent)
    }

    fn validate_destination(
        &self,
        allowed_dir: &Path,
        destination: &Path,
        label: &str,
    ) -> Result<()> {
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
        self.validate_destination(allowed_dir, destination, "write destination")?;
        self.prepare_optional(destination, create_parents)
    }

    fn for_link(&self, allowed_dir: &Path, destination: &Path) -> Result<ConfinedFile> {
        self.validate_destination(allowed_dir, destination, "link destination")?;
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        reject_symlink_components(&self.root_path, parent)?;
        let directory = self.open_dir(parent, true)?;
        let name = destination.file_name().ok_or_else(|| {
            CrivError::new(format!(
                "repository link path {} has no name",
                destination.display()
            ))
        })?;
        Ok(ConfinedFile {
            parent: directory,
            name: name.to_os_string(),
            relative: destination.to_path_buf(),
        })
    }

    fn prepare_optional(
        &self,
        relative: &Path,
        create_parents: bool,
    ) -> Result<Option<ConfinedFile>> {
        reject_symlink_components(&self.root_path, relative)?;
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

    fn open_dir_optional(&self, relative: &Path, create: bool) -> Result<Option<Dir>> {
        reject_symlink_components(&self.root_path, relative)?;
        let mut directory = self.root.try_clone()?;
        for component in relative.components() {
            let std::path::Component::Normal(part) = component else {
                continue;
            };
            directory = match directory.open_dir_nofollow(part) {
                Ok(next) => next,
                Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                    match directory.create_dir(part) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
                    }
                    directory.open_dir_nofollow(part)?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
        }
        Ok(Some(directory))
    }
}

struct ConfinedFile {
    parent: Dir,
    name: OsString,
    relative: PathBuf,
}

impl ConfinedFile {
    fn open_regular(&self, write: bool) -> Result<Option<cap_std::fs::File>> {
        let mut options = nofollow_options();
        options.read(true).write(write);
        match self.parent.open_with(&self.name, &options) {
            Ok(file) if file.metadata()?.is_file() => Ok(Some(file)),
            Ok(_) => Err(CrivError::new(format!(
                "vault path {} must be a regular file",
                self.relative.display()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn open_required_regular(&self) -> Result<cap_std::fs::File> {
        self.open_regular(false)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault file {} does not exist", self.relative.display()),
            )
            .into()
        })
    }

    fn read_optional_string(&self) -> Result<Option<String>> {
        let Some(mut file) = self.open_regular(false)? else {
            return Ok(None);
        };
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(Some(contents))
    }

    fn write_atomic(&self, contents: &str, permissions: Option<CapPermissions>) -> Result<()> {
        self.write_atomic_bytes(contents.as_bytes(), permissions)
    }

    fn write_atomic_bytes(
        &self,
        contents: &[u8],
        permissions: Option<CapPermissions>,
    ) -> Result<()> {
        let inherited_permissions = match permissions {
            Some(permissions) => Some(permissions),
            None => self
                .open_regular(false)?
                .map(|file| file.metadata().map(|metadata| metadata.permissions()))
                .transpose()?,
        };
        let file_name = self.name.to_string_lossy();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        for attempt in 0..100 {
            let temp_name = OsString::from(format!(
                ".{file_name}.{}.{}.tmp",
                std::process::id(),
                nonce + attempt
            ));
            let mut options = nofollow_options();
            options.write(true).create_new(true);
            let mut file = match self.parent.open_with(&temp_name, &options) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            let result = (|| -> std::io::Result<()> {
                file.write_all(contents)?;
                file.sync_all()?;
                if let Some(permissions) = inherited_permissions.clone() {
                    file.set_permissions(permissions)?;
                }
                self.parent.rename(&temp_name, &self.parent, &self.name)?;
                Ok(())
            })();
            if let Err(error) = result {
                let _ = self.parent.remove_file(&temp_name);
                return Err(error.into());
            }
            sync_directory_handle(&self.parent)?;
            return Ok(());
        }

        Err(CrivError::new(format!(
            "failed to create temporary file for {}",
            self.relative.display()
        )))
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
    directory.try_clone()?.into_std_file().sync_all()?;
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

fn reject_symlink_components(root: &Path, destination: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    let components = destination
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CrivError::new(format!(
                    "refusing to write through symlinked vault path component {}",
                    current.display()
                )));
            }
            Ok(metadata) if index + 1 < components.len() && !metadata.is_dir() => {
                return Err(CrivError::new(format!(
                    "cannot create vault path below non-directory component {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn relative_link_target(destination: &Path, target: &Path) -> PathBuf {
    let depth = destination
        .parent()
        .map(|parent| parent.components().count())
        .unwrap_or(0);
    let mut relative = PathBuf::new();
    for _ in 0..depth {
        relative.push("..");
    }
    relative.push(target);
    relative
}

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions, Permissions as CapPermissions};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{CrivError, Result};

#[cfg(test)]
pub(crate) fn copy_fixture_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_fixture_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Create `destination` and its missing parents, under the same confinement
/// rules as [`write_atomic_in`]. Directory creation gets the same treatment as
/// file writes because a symlinked component would otherwise let scaffolding
/// materialize outside the vault before anything is written into it.
pub(crate) fn create_dir_in(root: &Path, destination: &Path) -> Result<()> {
    validate_relative_path("directory destination", destination)?;
    open_confined_dir(root, destination, true)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum LinkOutcome {
    Unchanged,
    Created,
    Replaced,
    DirectoryInTheWay,
    Unsupported,
}

/// Point `destination` at `target`, both vault-relative. Governed by ADR-0053.
pub(crate) fn link_dir_in(
    root: &Path,
    destination: &Path,
    target: &Path,
    replace_directory: bool,
) -> Result<LinkOutcome> {
    validate_relative_path("link destination", destination)?;
    validate_relative_path("link target", target)?;
    let root = fs::canonicalize(root).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve vault root {} for confined link: {err}",
            root.display()
        ))
    })?;

    if let Some(parent) = destination.parent() {
        reject_symlink_components(&root, parent)?;
    }

    let link_path = root.join(destination);
    let target_path = root.join(target);
    let relative_target = relative_link_target(destination, target);

    match fs::symlink_metadata(&link_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let current = fs::read_link(&link_path).unwrap_or_default();
            let resolves_to_target = fs::canonicalize(&link_path)
                .ok()
                .zip(fs::canonicalize(&target_path).ok())
                .is_some_and(|(link, target)| link == target);
            if current == relative_target || resolves_to_target {
                return Ok(LinkOutcome::Unchanged);
            }
            remove_dir_link(&link_path)?;
        }
        Ok(metadata) if metadata.is_dir() => {
            if !replace_directory {
                return Ok(LinkOutcome::DirectoryInTheWay);
            }
            fs::remove_dir_all(&link_path)?;
            return finish_link(&root, destination, &relative_target, &target_path).map(
                |created| {
                    if created {
                        LinkOutcome::Replaced
                    } else {
                        LinkOutcome::Unsupported
                    }
                },
            );
        }
        Ok(_) => fs::remove_file(&link_path)?,
        Err(_) => {}
    }

    finish_link(&root, destination, &relative_target, &target_path).map(|created| {
        if created {
            LinkOutcome::Created
        } else {
            LinkOutcome::Unsupported
        }
    })
}

/// Returns false when the platform refuses to create the link.
fn finish_link(
    root: &Path,
    destination: &Path,
    relative_target: &Path,
    absolute_target: &Path,
) -> Result<bool> {
    if let Some(parent) = destination.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_in(root, parent)?;
    }
    let link_path = root.join(destination);

    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(relative_target, &link_path);
    #[cfg(windows)]
    let result = junction::create(absolute_target, &link_path);
    #[cfg(not(any(unix, windows)))]
    let result = Err(std::io::Error::other(
        "links are unsupported on this platform",
    ));

    #[cfg(not(windows))]
    let _ = absolute_target;
    #[cfg(not(unix))]
    let _ = relative_target;

    match result {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Remove a directory link without traversing into its target.
pub(crate) fn remove_dir_link(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if junction::exists(path).unwrap_or(false) {
            junction::delete(path)?;
        }
        fs::remove_dir(path)
    }
    #[cfg(not(windows))]
    {
        fs::remove_file(path)
    }
}

/// Path from the link's directory to the target.
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

/// Write `destination` only if it does not already exist, under the same
/// confinement rules as [`write_atomic_in`]. Returns whether the file was
/// created.
pub(crate) fn write_new_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &str,
) -> Result<bool> {
    let target = ConfinedFile::for_write(root, allowed_dir, destination)?;
    if target.open_regular(false)?.is_some() {
        return Ok(false);
    }
    target.write_atomic(contents, None)?;
    Ok(true)
}

/// Append `line` to `destination` unless it is already present, under the same
/// confinement rules as [`write_atomic_in`].
pub(crate) fn append_line_if_missing_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    line: &str,
) -> Result<()> {
    let target = ConfinedFile::for_write(root, allowed_dir, destination)?;
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

/// Atomically write a vault-controlled file without following symlinks.
///
/// Both `allowed_dir` and `destination` are paths relative to `root`, and the
/// destination must be inside the allowed directory. Keeping validation and
/// publication in one operation prevents callers from validating one path and
/// then accidentally writing another.
pub(crate) fn write_atomic_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &str,
) -> Result<()> {
    ConfinedFile::for_write(root, allowed_dir, destination)?.write_atomic(contents, None)
}

pub(crate) fn file_permissions_in(root: &Path, source: &Path) -> Result<fs::Permissions> {
    let target = ConfinedFile::for_read(root, source)?;
    let file = target.open_required_regular()?;
    let permissions = file.metadata()?.permissions();
    let file = file.into_std();
    Ok(permissions.into_std(&file)?)
}

pub(crate) fn write_atomic_with_permissions_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &str,
    permissions: fs::Permissions,
) -> Result<()> {
    ConfinedFile::for_write(root, allowed_dir, destination)?
        .write_atomic(contents, Some(CapPermissions::from_std(permissions)))
}

pub(crate) fn write_atomic_bytes_with_permissions_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &[u8],
    permissions: fs::Permissions,
) -> Result<()> {
    ConfinedFile::for_write(root, allowed_dir, destination)?
        .write_atomic_bytes(contents, Some(CapPermissions::from_std(permissions)))
}

/// Like [`write_atomic_in`], but leaves an identical existing file untouched.
#[cfg(test)]
pub(crate) fn write_atomic_if_changed_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &str,
) -> Result<bool> {
    let target = ConfinedFile::for_write(root, allowed_dir, destination)?;
    if target.read_optional_string()?.as_deref() == Some(contents) {
        return Ok(false);
    }
    target.write_atomic(contents, None)?;
    Ok(true)
}

/// Remove a vault-controlled file without following symlinks. Callers use this
/// after publishing replacement files so a failed reconciliation remains
/// recoverable from its newly written destinations.
pub(crate) fn remove_file_in(root: &Path, allowed_dir: &Path, destination: &Path) -> Result<()> {
    ConfinedFile::for_write(root, allowed_dir, destination)?.remove()
}

pub(crate) fn rename_file_in(
    root: &Path,
    allowed_dir: &Path,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    let source = ConfinedFile::for_write(root, allowed_dir, source)?;
    let destination = ConfinedFile::for_write(root, allowed_dir, destination)?;
    source.rename_to(&destination)
}

/// Open or create one persistent regular file without following the final path.
pub(crate) fn open_regular_file_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
) -> Result<(PathBuf, fs::File)> {
    let target = ConfinedFile::for_write(root, allowed_dir, destination)?;
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
    Ok((root.join(destination), file.into_std()))
}

/// Read one regular repository file through no-follow directory handles.
pub(crate) fn read_file_in(root: &Path, source: &Path) -> Result<Vec<u8>> {
    let target = ConfinedFile::for_read(root, source)?;
    let mut file = target.open_required_regular()?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

pub(crate) fn read_file_with_permissions_in(
    root: &Path,
    source: &Path,
) -> Result<(Vec<u8>, fs::Permissions)> {
    let target = ConfinedFile::for_read(root, source)?;
    let mut file = target.open_required_regular()?;
    let permissions = file.metadata()?.permissions();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    let file = file.into_std();
    Ok((contents, permissions.into_std(&file)?))
}

pub(crate) fn read_optional_file_with_permissions_in(
    root: &Path,
    source: &Path,
) -> Result<Option<(Vec<u8>, fs::Permissions)>> {
    let Some(target) = ConfinedFile::for_read_optional(root, source)? else {
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

pub(crate) fn read_file_with_metadata_in(
    root: &Path,
    source: &Path,
) -> Result<(Vec<u8>, fs::Metadata)> {
    let target = ConfinedFile::for_read(root, source)?;
    let mut file = target.open_required_regular()?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    let file = file.into_std();
    Ok((contents, file.metadata()?))
}

pub(crate) fn read_to_string_in(root: &Path, source: &Path) -> Result<String> {
    let target = ConfinedFile::for_read(root, source)?;
    target
        .read_optional_string()?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).into())
}

pub(crate) fn read_optional_to_string_in(root: &Path, source: &Path) -> Result<Option<String>> {
    let Some(target) = ConfinedFile::for_read_optional(root, source)? else {
        return Ok(None);
    };
    target.read_optional_string()
}

pub(crate) fn file_exists_in(root: &Path, source: &Path) -> Result<bool> {
    let Some(target) = ConfinedFile::for_read_optional(root, source)? else {
        return Ok(false);
    };
    Ok(target.open_regular(false)?.is_some())
}

pub(crate) fn directory_exists_in(root: &Path, source: &Path) -> Result<bool> {
    validate_relative_path("directory source", source)?;
    Ok(open_confined_dir_optional(root, source, false)?.is_some())
}

pub(crate) fn read_dir_names_in(root: &Path, source: &Path) -> Result<Option<Vec<OsString>>> {
    validate_relative_path("directory source", source)?;
    let Some(directory) = open_confined_dir_optional(root, source, false)? else {
        return Ok(None);
    };
    let mut names = Vec::new();
    for entry in directory.entries()? {
        names.push(entry?.file_name());
    }
    Ok(Some(names))
}

pub(crate) fn remove_empty_dir_in(root: &Path, source: &Path) -> Result<()> {
    let Some(target) = ConfinedFile::for_read_optional(root, source)? else {
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

struct ConfinedFile {
    parent: Dir,
    name: OsString,
    relative: PathBuf,
}

impl ConfinedFile {
    fn for_read(root: &Path, source: &Path) -> Result<Self> {
        Self::for_read_optional(root, source)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault file {} does not exist", source.display()),
            )
            .into()
        })
    }

    fn for_read_optional(root: &Path, source: &Path) -> Result<Option<Self>> {
        validate_relative_path("read source", source)?;
        Self::prepare_optional(root, source, false)
    }

    fn for_write(root: &Path, allowed_dir: &Path, destination: &Path) -> Result<Self> {
        validate_relative_path("allowed write directory", allowed_dir)?;
        validate_relative_path("write destination", destination)?;
        if allowed_dir != Path::new(".") && !destination.starts_with(allowed_dir) {
            return Err(CrivError::new(format!(
                "refusing to write {} outside allowed vault directory {}",
                destination.display(),
                allowed_dir.display()
            )));
        }
        Self::prepare_optional(root, destination, true)?.ok_or_else(|| {
            CrivError::new(format!(
                "failed to create parent for {}",
                destination.display()
            ))
        })
    }

    fn prepare_optional(
        root: &Path,
        relative: &Path,
        create_parents: bool,
    ) -> Result<Option<Self>> {
        let canonical_root = fs::canonicalize(root).map_err(|error| {
            CrivError::new(format!(
                "failed to resolve vault root {} for confined access: {error}",
                root.display()
            ))
        })?;
        reject_symlink_components(&canonical_root, relative)?;
        let parent = relative.parent().unwrap_or_else(|| Path::new("."));
        let Some(directory) = open_confined_dir_optional(root, parent, create_parents)? else {
            return Ok(None);
        };
        let name = relative.file_name().ok_or_else(|| {
            CrivError::new(format!(
                "repository file path {} has no name",
                relative.display()
            ))
        })?;
        Ok(Some(Self {
            parent: directory,
            name: name.to_os_string(),
            relative: relative.to_path_buf(),
        }))
    }

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

fn open_confined_dir(root: &Path, relative: &Path, create: bool) -> Result<Dir> {
    open_confined_dir_optional(root, relative, create)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("vault directory {} does not exist", relative.display()),
        )
        .into()
    })
}

fn open_confined_dir_optional(root: &Path, relative: &Path, create: bool) -> Result<Option<Dir>> {
    let canonical_root = fs::canonicalize(root).map_err(|error| {
        CrivError::new(format!(
            "failed to resolve vault root {} for confined access: {error}",
            root.display()
        ))
    })?;
    reject_symlink_components(&canonical_root, relative)?;
    let mut directory = Dir::open_ambient_dir(&canonical_root, ambient_authority())?;
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
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn normalize_rel(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn strip_prefix(path: &Path, root: &Path) -> String {
    normalize_rel(path.strip_prefix(root).unwrap_or(path))
}

pub(crate) fn kebab(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub(crate) fn is_adr_id(value: &str) -> bool {
    value.len() == 8
        && value.starts_with("ADR-")
        && value[4..].chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn find_wiki_links_with_lines(markdown: &str) -> Vec<(usize, String, Range<usize>)> {
    let mut in_code_block = false;
    let mut code_ranges = Vec::new();

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_ranges.push(range);
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                code_ranges.push(range);
            }
            Event::Code(_) => code_ranges.push(range),
            _ if in_code_block => code_ranges.push(range),
            _ => {}
        }
    }

    let mut links = Vec::new();
    let mut start = 0;
    while let Some(open) = markdown[start..].find("[[") {
        let open = start + open;
        let body_start = open + 2;
        if in_ranges(open, &code_ranges) {
            start = body_start;
            continue;
        }
        if let Some(close) = markdown[body_start..].find("]]") {
            let close = body_start + close;
            if !in_ranges(close, &code_ranges) {
                links.push((
                    line_number(markdown, open),
                    markdown[body_start..close].to_string(),
                    open..close + 2,
                ));
            }
            start = close + 2;
        } else {
            break;
        }
    }
    links
}

pub(crate) fn markdown_headings(markdown: &str) -> Vec<(usize, String, usize)> {
    let mut headings = Vec::new();
    let mut active: Option<(usize, usize, String)> = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                active = Some((
                    heading_level(level),
                    line_number(markdown, range.start),
                    String::new(),
                ));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, heading)) = &mut active {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, line, text)) = active.take() {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        headings.push((level, text, line));
                    }
                }
            }
            _ => {}
        }
    }
    headings
}

#[cfg(test)]
fn glob_matches(pattern: &str, value: &str) -> bool {
    let patterns = [pattern.to_string()];
    GlobMatcher::new(&patterns).is_ok_and(|matcher| matcher.is_match(value))
}

#[derive(Debug, Clone)]
pub(crate) struct GlobMatcher {
    sets: Vec<(GlobSet, Vec<usize>)>,
}

impl GlobMatcher {
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        Self::from_patterns(patterns, (0..patterns.len()).collect())
    }

    /// Compiles every valid pattern and preserves its original index. This is
    /// for legacy matching paths where an invalid glob has always meant
    /// "does not match", rather than a validation error.
    pub(crate) fn from_valid_patterns(patterns: &[String]) -> Self {
        let mut valid = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            if GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .is_ok()
            {
                valid.push((index, pattern.clone()));
            }
        }
        match Self::from_patterns(
            &valid
                .iter()
                .map(|(_, pattern)| pattern.clone())
                .collect::<Vec<_>>(),
            valid.iter().map(|(index, _)| *index).collect(),
        ) {
            Ok(matcher) => matcher,
            // A valid aggregate can exceed globset's automaton limit. Keep the
            // tolerant contract by compiling each valid pattern independently.
            Err(_) => Self {
                sets: valid
                    .iter()
                    .filter_map(|(index, pattern)| {
                        Self::from_patterns(std::slice::from_ref(pattern), vec![*index]).ok()
                    })
                    .flat_map(|matcher| matcher.sets)
                    .collect(),
            },
        }
    }

    fn from_patterns(patterns: &[String], pattern_indices: Vec<usize>) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .backslash_escape(true)
                    .build()
                    .map_err(|err| CrivError::new(format!("invalid glob `{pattern}`: {err}")))?,
            );
        }
        Ok(Self {
            sets: vec![(
                builder
                    .build()
                    .map_err(|err| CrivError::new(format!("failed to compile globs: {err}")))?,
                pattern_indices,
            )],
        })
    }

    pub(crate) fn is_match(&self, value: &str) -> bool {
        self.sets.iter().any(|(set, _)| set.is_match(value))
    }

    pub(crate) fn matching_pattern_indices_into(&self, value: &str, into: &mut Vec<usize>) {
        into.clear();
        let mut matched = Vec::new();
        for (set, pattern_indices) in &self.sets {
            // globset clears `matched` before every call, so it is safe to
            // reuse this scratch allocation while accumulating all sets.
            set.matches_into(value, &mut matched);
            into.extend(matched.iter().map(|index| pattern_indices[*index]));
        }
    }
}

fn in_ranges(byte_offset: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| byte_offset >= range.start && byte_offset < range.end)
}

fn line_number(markdown: &str, byte_offset: usize) -> usize {
    markdown[..byte_offset.min(markdown.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn heading_level(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_links_include_line_numbers() {
        let links = find_wiki_links_with_lines("a [[one]]\nb [[two|Two]]");
        assert_eq!(
            links,
            vec![(1, "one".into(), 2..9), (2, "two|Two".into(), 12..23)]
        );
    }

    #[test]
    fn wiki_links_ignore_code_examples() {
        let links = find_wiki_links_with_lines("`[[example]]`\n[[real]]\n```\n[[fenced]]\n```");
        assert_eq!(links, vec![(2, "real".into(), 14..22)]);
    }

    #[test]
    fn simple_globs_match_repo_paths() {
        assert!(glob_matches("src/**", "src/auth/verify.rs"));
        assert!(glob_matches("src/*.rs", "src/lib.rs"));
        assert!(!glob_matches("src/*.rs", "src/auth/lib.rs"));
    }

    #[test]
    fn atomic_write_replaces_existing_file_contents() {
        let root = std::env::temp_dir().join(format!(
            "criv-atomic-write-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        write_atomic_in(
            &root,
            Path::new("."),
            Path::new("state.json"),
            "{\"old\":true}\n",
        )
        .unwrap();
        write_atomic_in(
            &root,
            Path::new("."),
            Path::new("state.json"),
            "{\"new\":true}\n",
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"new\":true}\n");
        let leftovers = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(leftovers, vec!["state.json".to_string()]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn confined_atomic_write_creates_nested_directories_and_replaces_files() {
        let root = tempfile::TempDir::new().unwrap();
        let destination = Path::new("docs/generated/architecture.md");

        assert!(
            write_atomic_if_changed_in(root.path(), Path::new("docs"), destination, "first\n",)
                .unwrap()
        );
        assert!(
            !write_atomic_if_changed_in(root.path(), Path::new("docs"), destination, "first\n",)
                .unwrap()
        );
        write_atomic_in(root.path(), Path::new("docs"), destination, "second\n").unwrap();

        assert_eq!(
            fs::read_to_string(root.path().join(destination)).unwrap(),
            "second\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn confined_atomic_write_rejects_symlinked_components() {
        use std::os::unix::fs::symlink;

        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        symlink(outside.path(), root.path().join("docs")).unwrap();

        let error = write_atomic_in(
            root.path(),
            Path::new("docs"),
            Path::new("docs/generated/architecture.md"),
            "generated\n",
        )
        .unwrap_err();

        assert!(error.to_string().contains("symlinked vault path component"));
        assert!(!outside.path().join("generated/architecture.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn held_directory_handle_contains_operations_after_path_replacement() {
        use std::os::unix::fs::symlink;

        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/original.md"), "inside\n").unwrap();
        fs::write(root.path().join("docs/remove.md"), "remove\n").unwrap();
        fs::write(root.path().join("docs/rename.md"), "rename\n").unwrap();
        fs::write(outside.path().join("original.md"), "outside\n").unwrap();
        fs::write(outside.path().join("remove.md"), "outside remove\n").unwrap();
        fs::write(outside.path().join("renamed.md"), "outside rename\n").unwrap();

        let read_target =
            ConfinedFile::for_read(root.path(), Path::new("docs/original.md")).unwrap();
        let write_target = ConfinedFile::for_write(
            root.path(),
            Path::new("docs"),
            Path::new("docs/generated.md"),
        )
        .unwrap();
        let remove_target =
            ConfinedFile::for_write(root.path(), Path::new("docs"), Path::new("docs/remove.md"))
                .unwrap();
        let rename_source =
            ConfinedFile::for_write(root.path(), Path::new("docs"), Path::new("docs/rename.md"))
                .unwrap();
        let rename_destination =
            ConfinedFile::for_write(root.path(), Path::new("docs"), Path::new("docs/renamed.md"))
                .unwrap();

        fs::rename(root.path().join("docs"), root.path().join("held-docs")).unwrap();
        symlink(outside.path(), root.path().join("docs")).unwrap();

        assert_eq!(
            read_target.read_optional_string().unwrap().unwrap(),
            "inside\n"
        );
        write_target.write_atomic("generated\n", None).unwrap();
        remove_target.remove().unwrap();
        rename_source.rename_to(&rename_destination).unwrap();

        assert_eq!(
            fs::read_to_string(outside.path().join("original.md")).unwrap(),
            "outside\n"
        );
        assert!(!outside.path().join("generated.md").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("remove.md")).unwrap(),
            "outside remove\n"
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("renamed.md")).unwrap(),
            "outside rename\n"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("held-docs/generated.md")).unwrap(),
            "generated\n"
        );
        assert!(!root.path().join("held-docs/remove.md").exists());
        assert!(!root.path().join("held-docs/rename.md").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("held-docs/renamed.md")).unwrap(),
            "rename\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn final_symlink_replacement_is_never_followed() {
        use std::os::unix::fs::symlink;

        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(outside.path().join("target.md"), "outside\n").unwrap();
        let target =
            ConfinedFile::for_write(root.path(), Path::new("docs"), Path::new("docs/target.md"))
                .unwrap();
        symlink(
            outside.path().join("target.md"),
            root.path().join("docs/target.md"),
        )
        .unwrap();

        assert!(target.write_atomic("changed\n", None).is_err());
        assert!(target.remove().is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("target.md")).unwrap(),
            "outside\n"
        );
    }
}

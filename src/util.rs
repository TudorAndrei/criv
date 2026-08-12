use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{CrivError, Result};

pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?)
}

pub(crate) fn is_text_file(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::with_capacity(8192);
    Read::by_ref(&mut file)
        .take(8192)
        .read_to_end(&mut buffer)?;
    Ok(content_inspector::inspect(&buffer).is_text())
}

/// Create `destination` and its missing parents, under the same confinement
/// rules as [`write_atomic_in`]. Directory creation gets the same treatment as
/// file writes because a symlinked component would otherwise let scaffolding
/// materialize outside the vault before anything is written into it.
pub(crate) fn create_dir_in(root: &Path, destination: &Path) -> Result<()> {
    validate_relative_path("directory destination", destination)?;
    let root = fs::canonicalize(root).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve vault root {} for confined write: {err}",
            root.display()
        ))
    })?;
    reject_symlink_components(&root, destination)?;
    fs::create_dir_all(root.join(destination))?;
    // Recheck: `create_dir_all` may have raced with a symlink being planted.
    reject_symlink_components(&root, destination)
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
    // Resolve first so an existence check never follows a symlinked path out of
    // the vault; `prepare_confined_write` rejects symlink components outright.
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    if path.exists() {
        return Ok(false);
    }
    write_atomic_ready(&path, contents)?;
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
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    let mut contents = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };

    if !contents.lines().any(|existing| existing.trim() == line) {
        if !contents.is_empty() && !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(line);
        contents.push('\n');
        write_atomic_ready(&path, &contents)?;
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
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    write_atomic_ready(&path, contents)
}

pub(crate) fn file_permissions_in(root: &Path, source: &Path) -> Result<fs::Permissions> {
    let source_path = prepare_confined_write(root, Path::new("."), source)?;
    let metadata = fs::symlink_metadata(source_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CrivError::new(format!(
            "refusing to inherit file permissions from non-file vault path {}",
            source.display()
        )));
    }
    Ok(metadata.permissions())
}

pub(crate) fn write_atomic_with_permissions_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &str,
    permissions: fs::Permissions,
) -> Result<()> {
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    write_atomic_ready_with_permissions(&path, contents, Some(permissions))
}

/// Like [`write_atomic_in`], but leaves an identical existing file untouched.
#[cfg(test)]
pub(crate) fn write_atomic_if_changed_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
    contents: &str,
) -> Result<bool> {
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    if path.exists() && fs::read_to_string(&path)? == contents {
        return Ok(false);
    }
    write_atomic_ready(&path, contents)?;
    Ok(true)
}

/// Remove a vault-controlled file without following symlinks. Callers use this
/// after publishing replacement files so a failed reconciliation remains
/// recoverable from its newly written destinations.
pub(crate) fn remove_file_in(root: &Path, allowed_dir: &Path, destination: &Path) -> Result<()> {
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CrivError::new(format!(
            "refusing to remove non-file vault path {}",
            destination.display()
        )));
    }
    fs::remove_file(path)?;
    sync_parent_directory(&root.join(destination))?;
    Ok(())
}

pub(crate) fn rename_file_in(
    root: &Path,
    allowed_dir: &Path,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    let source_path = prepare_confined_write(root, allowed_dir, source)?;
    let destination_path = prepare_confined_write(root, allowed_dir, destination)?;
    let metadata = fs::symlink_metadata(&source_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CrivError::new(format!(
            "refusing to move non-file vault path {}",
            source.display()
        )));
    }
    let source_parent = source_path.parent().map(Path::to_path_buf);
    fs::rename(source_path, &destination_path)?;
    if let Some(source_parent) = source_parent {
        sync_directory(&source_parent)?;
    }
    sync_parent_directory(&destination_path)?;
    Ok(())
}

/// Open or create one persistent regular file without following the final path.
pub(crate) fn open_regular_file_in(
    root: &Path,
    allowed_dir: &Path,
    destination: &Path,
) -> Result<(PathBuf, fs::File)> {
    let path = prepare_confined_write(root, allowed_dir, destination)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(&path)?;
    if !file.metadata()?.is_file() {
        return Err(CrivError::new(format!(
            "vault path {} must be a regular file",
            destination.display()
        )));
    }
    Ok((path, file))
}

fn write_atomic_ready(path: &Path, contents: &str) -> Result<()> {
    let permissions = fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .map(|metadata| metadata.permissions());
    write_atomic_ready_with_permissions(path, contents, permissions)
}

fn write_atomic_ready_with_permissions(
    path: &Path,
    contents: &str,
    permissions: Option<fs::Permissions>,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| CrivError::new(format!("cannot write atomic file at {}", path.display())))?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    for attempt in 0..100 {
        let temp_path = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            nonce + attempt
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        };

        let write_result = file
            .write_all(contents.as_bytes())
            .and_then(|_| file.sync_all());
        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }
        if let Some(permissions) = &permissions
            && let Err(err) = fs::set_permissions(&temp_path, permissions.clone())
        {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }
        if let Err(err) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }
        sync_parent_directory(path)?;
        return Ok(());
    }

    Err(CrivError::new(format!(
        "failed to create temporary file for {}",
        path.display()
    )))
}

fn sync_parent_directory(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let directory = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?
    };
    #[cfg(not(windows))]
    let directory = fs::File::open(path)?;
    directory.sync_all()?;
    Ok(())
}

fn prepare_confined_write(root: &Path, allowed_dir: &Path, destination: &Path) -> Result<PathBuf> {
    validate_relative_path("allowed write directory", allowed_dir)?;
    validate_relative_path("write destination", destination)?;
    if allowed_dir != Path::new(".") && !destination.starts_with(allowed_dir) {
        return Err(CrivError::new(format!(
            "refusing to write {} outside allowed vault directory {}",
            destination.display(),
            allowed_dir.display()
        )));
    }

    let root = fs::canonicalize(root).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve vault root {} for confined write: {err}",
            root.display()
        ))
    })?;
    reject_symlink_components(&root, destination)?;

    let path = root.join(destination);
    let parent = path
        .parent()
        .ok_or_else(|| CrivError::new(format!("cannot write atomic file at {}", path.display())))?;
    fs::create_dir_all(parent)?;

    // Recheck after directory creation: a pre-existing or concurrently placed
    // symlink must never become the parent of our temporary file.
    reject_symlink_components(&root, destination)?;
    let allowed = root.join(allowed_dir);
    let allowed = fs::canonicalize(&allowed).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve allowed vault directory {}: {err}",
            allowed.display()
        ))
    })?;
    let resolved_parent = fs::canonicalize(parent).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve write parent {}: {err}",
            parent.display()
        ))
    })?;
    if !resolved_parent.starts_with(&allowed) {
        return Err(CrivError::new(format!(
            "refusing to write {} outside resolved allowed vault directory {}",
            path.display(),
            allowed.display()
        )));
    }

    Ok(path)
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

pub(crate) fn walk_files(
    vault_root: &Path,
    walk_root: &Path,
    extension: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let relative = walk_root.strip_prefix(vault_root).map_err(|_| {
        CrivError::new(format!(
            "vault walk root {} is outside vault root {}",
            walk_root.display(),
            vault_root.display()
        ))
    })?;
    let mut current = vault_root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            current.push(component.as_os_str());
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CrivError::new(format!(
                    "refusing to read symlinked vault path {}",
                    current.display()
                )));
            }
            Ok(metadata) if current != walk_root && !metadata.is_dir() => {
                return Err(CrivError::new(format!(
                    "vault path component {} must be a real directory",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
            Err(err) => return Err(err.into()),
        }
    }
    match fs::symlink_metadata(walk_root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CrivError::new(format!(
                "refusing to read symlinked vault path {}",
                walk_root.display()
            )));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(CrivError::new(format!(
                "vault walk root {} must be a real directory",
                walk_root.display()
            )));
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(err) => return Err(err.into()),
    }
    walk_files_inner(walk_root, extension, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_files_inner(root: &Path, extension: Option<&str>, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(CrivError::new(format!(
                "refusing to read symlinked vault path {}",
                path.display()
            )));
        }
        if name == ".git" || name == ".criv" || name == "target" || name == "node_modules" {
            continue;
        }

        if file_type.is_dir() {
            walk_files_inner(&path, extension, files)?;
        } else if file_type.is_file()
            && extension.is_none_or(|ext| path.extension().is_some_and(|value| value == ext))
        {
            files.push(path);
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

pub(crate) fn find_wiki_links_with_lines(markdown: &str) -> Vec<(usize, String)> {
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
        assert_eq!(links, vec![(1, "one".into()), (2, "two|Two".into())]);
    }

    #[test]
    fn wiki_links_ignore_code_examples() {
        let links = find_wiki_links_with_lines("`[[example]]`\n[[real]]\n```\n[[fenced]]\n```");
        assert_eq!(links, vec![(2, "real".into())]);
    }

    #[test]
    fn simple_globs_match_repo_paths() {
        assert!(glob_matches("src/**", "src/auth/verify.rs"));
        assert!(glob_matches("src/*.rs", "src/lib.rs"));
        assert!(!glob_matches("src/*.rs", "src/auth/lib.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn vault_walk_rejects_symlinked_files() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join("linked.md")).unwrap();

        let error = walk_files(root.path(), root.path(), Some("md")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing to read symlinked vault path")
        );
    }

    #[cfg(unix)]
    #[test]
    fn vault_walk_rejects_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("linked-docs")).unwrap();

        let error = walk_files(root.path(), root.path(), Some("md")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing to read symlinked vault path")
        );
    }

    #[cfg(unix)]
    #[test]
    fn vault_walk_rejects_an_exact_symlinked_root() {
        use std::os::unix::fs::symlink;

        let repository = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), repository.path().join("docs")).unwrap();

        let error = walk_files(
            repository.path(),
            &repository.path().join("docs"),
            Some("md"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("symlinked vault path"));
    }

    #[cfg(unix)]
    #[test]
    fn vault_walk_rejects_a_symlinked_ancestor_of_the_walk_root() {
        use std::os::unix::fs::symlink;

        let repository = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("docs")).unwrap();
        symlink(outside.path(), repository.path().join("linked-parent")).unwrap();

        let error = walk_files(
            repository.path(),
            &repository.path().join("linked-parent/docs"),
            Some("md"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("symlinked vault path"));
    }

    #[cfg(windows)]
    #[test]
    fn vault_walk_rejects_a_windows_junction() {
        let repository = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        junction::create(outside.path(), repository.path().join("docs")).unwrap();

        let error = walk_files(
            repository.path(),
            &repository.path().join("docs"),
            Some("md"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("symlinked vault path"));
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
}

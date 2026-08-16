use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use serde::Serialize;

const INVENTORY_SCHEMA: &str = "criv.discovery-inventory.v1";

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-inventory",
    about = "Create a content-addressed identity for an observed discovery workload"
)]
struct Args {
    /// Git worktree to inventory.
    #[arg(long)]
    root: PathBuf,
    /// Stable workload name.
    #[arg(long)]
    workload_id: String,
    /// Local full inventory output. It must not exist.
    #[arg(long)]
    output: PathBuf,
    /// Sanitized summary output. It must not exist.
    #[arg(long)]
    summary_output: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize)]
struct EncodedPath {
    encoding: &'static str,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum EntryKind {
    Directory,
    File,
    Link,
}

#[derive(Debug, Clone, Serialize)]
struct EntryIdentity {
    path: EncodedPath,
    kind: EntryKind,
    size: u64,
    executable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_target: Option<EncodedPath>,
}

#[derive(Debug, Clone, Serialize)]
struct GitIdentity {
    head: String,
    clean: bool,
    status_digest: String,
    index_digest: Option<String>,
    info_exclude_digest: Option<String>,
    global_exclude_path: Option<String>,
    global_exclude_digest: Option<String>,
    core_ignorecase: Option<String>,
    core_symlinks: Option<String>,
    submodule_status_digest: String,
}

#[derive(Debug, Clone, Default, Serialize)]
struct InventorySummary {
    directories: u64,
    files: u64,
    links: u64,
    git_directories_excluded: u64,
    hidden_entries: u64,
    logical_file_bytes: u64,
    maximum_depth: usize,
    extensions: BTreeMap<String, u64>,
    top_level: BTreeMap<String, SubtreeSummary>,
    git_paths: GitPathSummary,
}

#[derive(Debug, Clone, Default, Serialize)]
struct SubtreeSummary {
    entries: u64,
    directories: u64,
    files: u64,
    links: u64,
    logical_file_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct GitPathSummary {
    tracked_paths: u64,
    tracked_path_digest: String,
    ignored_paths: u64,
    ignored_path_digest: String,
    ignored_directories: u64,
    ignored_directory_digest: String,
}

#[derive(Debug, Clone, Default, Serialize)]
struct SelectionShape {
    source_roots: Vec<String>,
    source_excludes: Vec<String>,
    vault_docs: Option<String>,
    rumdl_includes: Vec<String>,
    rumdl_excludes: Vec<String>,
    rumdl_respect_gitignore: Option<bool>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct IdentityPayload<'a> {
    schema: &'static str,
    git: &'a GitIdentity,
    entries: &'a [EntryIdentity],
}

#[derive(Debug, Serialize)]
struct FullInventory {
    schema: &'static str,
    workload_id: String,
    workload_digest: String,
    git: GitIdentity,
    selection_shape: SelectionShape,
    summary: InventorySummary,
    entries: Vec<EntryIdentity>,
}

#[derive(Debug, Serialize)]
struct SanitizedSummary<'a> {
    schema: &'static str,
    workload_id: &'a str,
    workload_digest: &'a str,
    head: &'a str,
    clean: bool,
    selection_shape: &'a SelectionShape,
    summary: &'a InventorySummary,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-inventory: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    if args.workload_id.trim().is_empty() {
        return Err("workload-id must not be empty".into());
    }
    ensure_new_output(&args.output)?;
    ensure_new_output(&args.summary_output)?;
    let root = fs::canonicalize(&args.root)
        .map_err(|error| format!("failed to resolve root {}: {error}", args.root.display()))?;
    if !root.is_dir() {
        return Err(format!("root is not a directory: {}", root.display()));
    }

    let git = git_identity(&root)?;
    let (entries, mut summary) = inventory_entries(&root)?;
    summary.git_paths = git_path_summary(&root)?;
    let selection_shape = selection_shape(&root);
    let workload_digest = identity_digest(&git, &entries)?;
    let inventory = FullInventory {
        schema: INVENTORY_SCHEMA,
        workload_id: args.workload_id,
        workload_digest,
        git,
        selection_shape,
        summary,
        entries,
    };
    let summary = SanitizedSummary {
        schema: INVENTORY_SCHEMA,
        workload_id: &inventory.workload_id,
        workload_digest: &inventory.workload_digest,
        head: &inventory.git.head,
        clean: inventory.git.clean,
        selection_shape: &inventory.selection_shape,
        summary: &inventory.summary,
    };
    write_json_new(&args.output, &inventory)?;
    if let Err(error) = write_json_new(&args.summary_output, &summary) {
        let _ = fs::remove_file(&args.output);
        return Err(error);
    }
    println!("{}", inventory.workload_digest);
    Ok(())
}

fn inventory_entries(root: &Path) -> Result<(Vec<EntryIdentity>, InventorySummary), String> {
    let mut entries = Vec::new();
    let mut summary = InventorySummary::default();
    walk_entries(root, root, &mut entries, &mut summary)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((entries, summary))
}

fn walk_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<EntryIdentity>,
    summary: &mut InventorySummary,
) -> Result<(), String> {
    let mut children = fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(display_error)?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("failed to make {} relative: {error}", path.display()))?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("failed to read metadata for {}: {error}", path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_dir() && child.file_name() == ".git" {
            summary.git_directories_excluded += 1;
            continue;
        }
        let depth = relative.components().count();
        summary.maximum_depth = summary.maximum_depth.max(depth);
        if file_type.is_symlink() {
            summary.links += 1;
            record_shape(summary, relative, ShapeKind::Link, 0);
            entries.push(EntryIdentity {
                path: encode_relative(relative),
                kind: EntryKind::Link,
                size: metadata.len(),
                executable: false,
                content_digest: None,
                link_target: Some(encode_os_string(
                    &fs::read_link(&path).map_err(display_error)?,
                )),
            });
        } else if file_type.is_dir() {
            summary.directories += 1;
            record_shape(summary, relative, ShapeKind::Directory, 0);
            entries.push(EntryIdentity {
                path: encode_relative(relative),
                kind: EntryKind::Directory,
                size: 0,
                executable: executable(&metadata),
                content_digest: None,
                link_target: None,
            });
            walk_entries(root, &path, entries, summary)?;
        } else if file_type.is_file() {
            summary.files += 1;
            summary.logical_file_bytes = summary.logical_file_bytes.saturating_add(metadata.len());
            record_shape(summary, relative, ShapeKind::File, metadata.len());
            *summary
                .extensions
                .entry(extension_group(relative))
                .or_default() += 1;
            entries.push(EntryIdentity {
                path: encode_relative(relative),
                kind: EntryKind::File,
                size: metadata.len(),
                executable: executable(&metadata),
                content_digest: Some(stream_digest(&path)?),
                link_target: None,
            });
        } else {
            return Err(format!(
                "unsupported filesystem entry in workload: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ShapeKind {
    Directory,
    File,
    Link,
}

fn record_shape(summary: &mut InventorySummary, relative: &Path, kind: ShapeKind, bytes: u64) {
    if is_hidden(relative) {
        summary.hidden_entries += 1;
    }
    let Some(first) = relative.components().next() else {
        return;
    };
    let key = encode_os_string(Path::new(first.as_os_str())).value;
    let subtree = summary.top_level.entry(key).or_default();
    subtree.entries += 1;
    subtree.logical_file_bytes = subtree.logical_file_bytes.saturating_add(bytes);
    match kind {
        ShapeKind::Directory => subtree.directories += 1,
        ShapeKind::File => subtree.files += 1,
        ShapeKind::Link => subtree.links += 1,
    }
}

#[cfg(unix)]
fn is_hidden(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.components().any(|component| {
        component
            .as_os_str()
            .as_bytes()
            .first()
            .is_some_and(|byte| *byte == b'.')
    })
}

#[cfg(windows)]
fn is_hidden(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|value| value.starts_with('.'))
    })
}

fn git_path_summary(root: &Path) -> Result<GitPathSummary, String> {
    let tracked = command_bytes(root, "git", &["ls-files", "-z"])?;
    let ignored = command_bytes(
        root,
        "git",
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let ignored_directories = command_bytes(
        root,
        "git",
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
            "-z",
        ],
    )?;
    Ok(GitPathSummary {
        tracked_paths: nul_path_count(&tracked),
        tracked_path_digest: bytes_digest(&tracked),
        ignored_paths: nul_path_count(&ignored),
        ignored_path_digest: bytes_digest(&ignored),
        ignored_directories: nul_path_count(&ignored_directories),
        ignored_directory_digest: bytes_digest(&ignored_directories),
    })
}

fn nul_path_count(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|byte| **byte == 0).count() as u64
}

fn selection_shape(root: &Path) -> SelectionShape {
    let mut shape = SelectionShape::default();
    match fs::read_to_string(root.join("criv.toml")) {
        Ok(contents) => match toml::from_str::<toml::Value>(&contents) {
            Ok(value) => {
                shape.source_roots = toml_strings(&value, &["source", "roots"]);
                shape.source_excludes = toml_strings(&value, &["source", "exclude"]);
                shape.vault_docs = toml_string(&value, &["vault", "docs"]);
            }
            Err(error) => shape.errors.push(format!("criv.toml: {error}")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => shape.errors.push(format!("criv.toml: {error}")),
    }
    match fs::read_to_string(root.join(".rumdl.toml")) {
        Ok(contents) => match toml::from_str::<toml::Value>(&contents) {
            Ok(value) => {
                shape.rumdl_includes = toml_strings(&value, &["global", "include"]);
                shape.rumdl_excludes = toml_strings(&value, &["global", "exclude"]);
                shape.rumdl_respect_gitignore = toml_bool(&value, &["global", "respect_gitignore"])
                    .or_else(|| toml_bool(&value, &["global", "respect-gitignore"]));
            }
            Err(error) => shape.errors.push(format!(".rumdl.toml: {error}")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => shape.errors.push(format!(".rumdl.toml: {error}")),
    }
    shape
}

fn toml_value_at<'a>(value: &'a toml::Value, path: &[&str]) -> Option<&'a toml::Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn toml_strings(value: &toml::Value, path: &[&str]) -> Vec<String> {
    toml_value_at(value, path)
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect()
}

fn toml_string(value: &toml::Value, path: &[&str]) -> Option<String> {
    toml_value_at(value, path)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

fn toml_bool(value: &toml::Value, path: &[&str]) -> Option<bool> {
    toml_value_at(value, path).and_then(toml::Value::as_bool)
}

fn git_identity(root: &Path) -> Result<GitIdentity, String> {
    let head = command_text(root, "git", &["rev-parse", "HEAD"])?;
    let status = command_bytes(root, "git", &["status", "--porcelain=v1", "-z"])?;
    let git_dir_text = command_text(root, "git", &["rev-parse", "--git-dir"])?;
    let git_dir = {
        let path = PathBuf::from(git_dir_text);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    };
    let global_exclude_path = optional_command_text(
        root,
        "git",
        &["config", "--path", "--get", "core.excludesfile"],
    );
    let global_exclude_digest = global_exclude_path
        .as_ref()
        .and_then(|path| optional_file_digest(Path::new(path)));
    let submodules =
        command_bytes(root, "git", &["submodule", "status", "--recursive"]).unwrap_or_default();
    Ok(GitIdentity {
        head,
        clean: status.is_empty(),
        status_digest: bytes_digest(&status),
        index_digest: optional_file_digest(&git_dir.join("index")),
        info_exclude_digest: optional_file_digest(&git_dir.join("info/exclude")),
        global_exclude_path,
        global_exclude_digest,
        core_ignorecase: optional_command_text(
            root,
            "git",
            &["config", "--get", "core.ignorecase"],
        ),
        core_symlinks: optional_command_text(root, "git", &["config", "--get", "core.symlinks"]),
        submodule_status_digest: bytes_digest(&submodules),
    })
}

fn identity_digest(git: &GitIdentity, entries: &[EntryIdentity]) -> Result<String, String> {
    serde_json::to_vec(&IdentityPayload {
        schema: INVENTORY_SCHEMA,
        git,
        entries,
    })
    .map(|bytes| bytes_digest(&bytes))
    .map_err(display_error)
}

fn stream_digest(path: &Path) -> Result<String, String> {
    let mut input = BufReader::new(
        File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?,
    );
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(display_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn extension_group(path: &Path) -> String {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|extension| extension.to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "<none-or-non-utf8>".into())
}

#[cfg(unix)]
fn encode_relative(path: &Path) -> EncodedPath {
    use std::os::unix::ffi::OsStrExt;

    encode_bytes(path.as_os_str().as_bytes())
}

#[cfg(unix)]
fn encode_os_string(path: &Path) -> EncodedPath {
    use std::os::unix::ffi::OsStrExt;

    encode_bytes(path.as_os_str().as_bytes())
}

#[cfg(unix)]
fn encode_bytes(bytes: &[u8]) -> EncodedPath {
    match std::str::from_utf8(bytes) {
        Ok(value) => EncodedPath {
            encoding: "utf8",
            value: value.replace('\\', "/"),
        },
        Err(_) => EncodedPath {
            encoding: "unix_bytes_hex",
            value: hex(bytes),
        },
    }
}

#[cfg(windows)]
fn encode_relative(path: &Path) -> EncodedPath {
    encode_windows(path.as_os_str())
}

#[cfg(windows)]
fn encode_os_string(path: &Path) -> EncodedPath {
    encode_windows(path.as_os_str())
}

#[cfg(windows)]
fn encode_windows(value: &OsStr) -> EncodedPath {
    use std::os::windows::ffi::OsStrExt;

    match value.to_str() {
        Some(value) => EncodedPath {
            encoding: "utf8",
            value: value.replace('\\', "/"),
        },
        None => {
            let bytes = value
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            EncodedPath {
                encoding: "windows_utf16le_hex",
                value: hex(&bytes),
            }
        }
    }
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn executable(_metadata: &fs::Metadata) -> bool {
    false
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn ensure_new_output(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("output already exists: {}", path.display()));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "output parent is not a directory: {}",
            parent.display()
        ));
    }
    Ok(())
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(display_error)?;
    let mut output = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut output, value).map_err(display_error)?;
    output.write_all(b"\n").map_err(display_error)
}

fn command_bytes(root: &Path, program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(display_error)?;
    if !output.status.success() {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn command_text(root: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    String::from_utf8(command_bytes(root, program, args)?)
        .map(|value| value.trim().to_string())
        .map_err(display_error)
}

fn optional_command_text(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    command_text(root, program, args)
        .ok()
        .filter(|value| !value.is_empty())
}

fn optional_file_digest(path: &Path) -> Option<String> {
    path.is_file().then(|| stream_digest(path).ok()).flatten()
}

fn bytes_digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_excludes_real_git_storage_directories() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("nested/.git")).unwrap();
        fs::write(root.path().join("nested/.git/index"), "private git state").unwrap();
        fs::write(root.path().join("nested/source.rs"), "fn main() {}\n").unwrap();

        let (entries, summary) = inventory_entries(root.path()).unwrap();

        assert_eq!(summary.git_directories_excluded, 1);
        assert_eq!(summary.files, 1);
        assert!(
            entries
                .iter()
                .all(|entry| entry.path.value != "nested/.git")
        );
    }

    #[test]
    fn inventory_excludes_git_storage() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join(".git/object"), "storage").unwrap();
        fs::write(root.path().join("visible.txt"), "visible").unwrap();

        let (entries, summary) = inventory_entries(root.path()).unwrap();

        assert_eq!(summary.files, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path.value, "visible.txt");
    }

    #[test]
    fn content_changes_inventory_identity() {
        let root = tempfile::TempDir::new().unwrap();
        let file = root.path().join("file.txt");
        fs::write(&file, "first").unwrap();
        let (first, _) = inventory_entries(root.path()).unwrap();
        fs::write(&file, "second").unwrap();
        let (second, _) = inventory_entries(root.path()).unwrap();
        assert_ne!(first[0].content_digest, second[0].content_digest);
    }

    #[test]
    fn summary_reports_hidden_and_top_level_shape() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/.hidden.rs"), "fn hidden() {}\n").unwrap();
        fs::write(root.path().join("README.md"), "# Readme\n").unwrap();

        let (_, summary) = inventory_entries(root.path()).unwrap();

        assert_eq!(summary.hidden_entries, 1);
        assert_eq!(summary.top_level["src"].files, 1);
        assert_eq!(summary.top_level["README.md"].files, 1);
    }

    #[test]
    fn selection_shape_reads_profile_authorities() {
        let root = tempfile::TempDir::new().unwrap();
        fs::write(
            root.path().join("criv.toml"),
            "[vault]\ndocs = \"knowledge\"\n[source]\nroots = [\"src\"]\nexclude = [\"vendor/**\"]\n",
        )
        .unwrap();
        fs::write(
            root.path().join(".rumdl.toml"),
            "[global]\ninclude = [\"docs/**\"]\nexclude = [\"drafts/**\"]\nrespect_gitignore = false\n",
        )
        .unwrap();

        let shape = selection_shape(root.path());

        assert_eq!(shape.source_roots, ["src"]);
        assert_eq!(shape.source_excludes, ["vendor/**"]);
        assert_eq!(shape.vault_docs.as_deref(), Some("knowledge"));
        assert_eq!(shape.rumdl_includes, ["docs/**"]);
        assert_eq!(shape.rumdl_excludes, ["drafts/**"]);
        assert_eq!(shape.rumdl_respect_gitignore, Some(false));
        assert!(shape.errors.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn link_target_is_identity_data() {
        use std::os::unix::fs::symlink;

        let root = tempfile::TempDir::new().unwrap();
        fs::write(root.path().join("a"), "a").unwrap();
        fs::write(root.path().join("b"), "b").unwrap();
        symlink("a", root.path().join("link")).unwrap();
        let (first, _) = inventory_entries(root.path()).unwrap();
        fs::remove_file(root.path().join("link")).unwrap();
        symlink("b", root.path().join("link")).unwrap();
        let (second, _) = inventory_entries(root.path()).unwrap();
        let first_link = first
            .iter()
            .find(|entry| entry.path.value == "link")
            .unwrap();
        let second_link = second
            .iter()
            .find(|entry| entry.path.value == "link")
            .unwrap();
        assert_ne!(first_link.link_target, second_link.link_target);
    }

    #[test]
    fn output_must_not_exist() {
        let root = tempfile::TempDir::new().unwrap();
        let output = root.path().join("inventory.json");
        fs::write(&output, "existing").unwrap();
        assert!(
            ensure_new_output(&output)
                .unwrap_err()
                .contains("already exists")
        );
    }
}

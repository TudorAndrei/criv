use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::Serialize;

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-snapshot",
    about = "Create one strict copy-on-write workload snapshot on APFS"
)]
struct Args {
    /// Existing workload directory to clone.
    #[arg(long)]
    source: PathBuf,
    /// New disposable directory. It must not exist.
    #[arg(long)]
    destination: PathBuf,
    /// Required physical free space before cloning.
    #[arg(long, default_value_t = 30)]
    minimum_free_gib: u64,
    /// Maximum physical allocation that directory metadata may consume.
    #[arg(long, default_value_t = 1)]
    maximum_allocation_gib: u64,
}

#[derive(Debug, Default, Serialize)]
struct CloneCounts {
    directories: u64,
    files: u64,
    links: u64,
    transient_git_entries_skipped: u64,
    logical_file_bytes: u64,
}

#[derive(Debug, Serialize)]
struct CloneReceipt {
    schema: &'static str,
    source: String,
    destination: String,
    free_bytes_before: u64,
    free_bytes_after: u64,
    allocated_bytes: u64,
    counts: CloneCounts,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-snapshot: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let (source, destination, destination_parent) = resolve_paths(&args.source, &args.destination)?;
    let minimum_free = args
        .minimum_free_gib
        .checked_mul(GIB)
        .ok_or_else(|| "minimum-free-gib is too large".to_string())?;
    let maximum_allocation = args
        .maximum_allocation_gib
        .checked_mul(GIB)
        .ok_or_else(|| "maximum-allocation-gib is too large".to_string())?;
    let free_before = free_bytes(&destination_parent)?;
    if free_before < minimum_free {
        return Err(format!(
            "{} has {:.2} GiB free; at least {} GiB is required",
            destination_parent.display(),
            free_before as f64 / GIB as f64,
            args.minimum_free_gib
        ));
    }

    let mut counts = CloneCounts::default();
    if let Err(error) = clone_tree(&source, &destination, &mut counts) {
        cleanup_created_destination(&destination);
        return Err(error);
    }
    let free_after = match free_bytes(&destination_parent) {
        Ok(value) => value,
        Err(error) => {
            cleanup_created_destination(&destination);
            return Err(error);
        }
    };
    let allocated_bytes = free_before.saturating_sub(free_after);
    if allocated_bytes > maximum_allocation {
        cleanup_created_destination(&destination);
        return Err(format!(
            "snapshot allocated {:.2} GiB, above the {} GiB safety limit; the disposable snapshot was removed",
            allocated_bytes as f64 / GIB as f64,
            args.maximum_allocation_gib
        ));
    }

    let receipt = CloneReceipt {
        schema: "criv.discovery-snapshot.v1",
        source: source.display().to_string(),
        destination: destination.display().to_string(),
        free_bytes_before: free_before,
        free_bytes_after: free_after,
        allocated_bytes,
        counts,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(display_error)?
    );
    Ok(())
}

fn resolve_paths(source: &Path, destination: &Path) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let source = fs::canonicalize(source)
        .map_err(|error| format!("failed to resolve source {}: {error}", source.display()))?;
    if !source.is_dir() {
        return Err(format!("source is not a directory: {}", source.display()));
    }
    if destination.exists() {
        return Err(format!(
            "destination already exists: {}",
            destination.display()
        ));
    }
    let file_name = destination
        .file_name()
        .ok_or_else(|| "destination must name a new directory".to_string())?;
    let parent = destination
        .parent()
        .ok_or_else(|| "destination must have a parent directory".to_string())?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        format!(
            "failed to resolve destination parent {}: {error}",
            parent.display()
        )
    })?;
    let destination = parent.join(file_name);
    if destination.starts_with(&source) {
        return Err("destination must not be inside the source tree".into());
    }
    Ok((source, destination, parent))
}

#[cfg(target_os = "macos")]
fn clone_tree(source: &Path, destination: &Path, counts: &mut CloneCounts) -> Result<(), String> {
    clone_tree_at(source, destination, counts, false)
}

#[cfg(target_os = "macos")]
fn clone_tree_at(
    source: &Path,
    destination: &Path,
    counts: &mut CloneCounts,
    inside_git_storage: bool,
) -> Result<(), String> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let metadata = fs::symlink_metadata(source).map_err(display_error)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        let target = fs::read_link(source).map_err(display_error)?;
        symlink(&target, destination).map_err(|error| {
            format!(
                "failed to clone link {} to {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
        counts.links += 1;
        return Ok(());
    }
    if file_type.is_file() {
        clone_file(source, destination)?;
        counts.files += 1;
        counts.logical_file_bytes = counts.logical_file_bytes.saturating_add(metadata.len());
        return Ok(());
    }
    if !file_type.is_dir() {
        if inside_git_storage {
            counts.transient_git_entries_skipped += 1;
            return Ok(());
        }
        return Err(format!(
            "unsupported filesystem entry in workload: {}",
            source.display()
        ));
    }

    fs::create_dir(destination).map_err(|error| {
        format!(
            "failed to create snapshot directory {}: {error}",
            destination.display()
        )
    })?;
    counts.directories += 1;
    let mut entries = fs::read_dir(source)
        .map_err(display_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(display_error)?;
    entries.sort_by_key(|entry| entry.file_name());
    let inside_git_storage = inside_git_storage || source.file_name() == Some(".git".as_ref());
    for entry in entries {
        clone_tree_at(
            &entry.path(),
            &destination.join(entry.file_name()),
            counts,
            inside_git_storage,
        )?;
    }
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(metadata.permissions().mode()),
    )
    .map_err(display_error)
}

#[cfg(target_os = "macos")]
fn clone_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    const CLONE_NOFOLLOW: u32 = 0x0001;
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| format!("source path contains NUL: {}", source.display()))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| format!("destination path contains NUL: {}", destination.display()))?;
    // SAFETY: both pointers are valid NUL-terminated paths for the duration of the call.
    let status = unsafe { libc::clonefile(source.as_ptr(), destination.as_ptr(), CLONE_NOFOLLOW) };
    if status == 0 {
        return Ok(());
    }
    Err(format!(
        "copy-on-write clone failed and no copy fallback was used: {}",
        std::io::Error::last_os_error()
    ))
}

#[cfg(not(target_os = "macos"))]
fn clone_tree(
    _source: &Path,
    _destination: &Path,
    _counts: &mut CloneCounts,
) -> Result<(), String> {
    Err("strict workload snapshots currently require macOS APFS clonefile support".into())
}

#[cfg(unix)]
fn free_bytes(path: &Path) -> Result<u64, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("path contains NUL: {}", path.display()))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated and stats is a valid output pointer.
    let status = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if status != 0 {
        return Err(format!(
            "failed to read free space: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: statvfs initialized stats on a zero return code.
    let stats = unsafe { stats.assume_init() };
    let bytes = (stats.f_bavail as u128).saturating_mul(stats.f_frsize as u128);
    Ok(bytes.min(u64::MAX as u128) as u64)
}

#[cfg(windows)]
fn free_bytes(_path: &Path) -> Result<u64, String> {
    Err("strict workload snapshots currently require macOS APFS clonefile support".into())
}

fn cleanup_created_destination(destination: &Path) {
    if let Err(error) = fs::remove_dir_all(destination) {
        eprintln!(
            "criv-discovery-snapshot: failed to remove disposable snapshot {}: {error}",
            destination.display()
        );
    }
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_destination_is_rejected() {
        let root = tempfile::TempDir::new().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        assert!(
            resolve_paths(&source, &destination)
                .unwrap_err()
                .contains("destination already exists")
        );
    }

    #[test]
    fn destination_inside_source_is_rejected() {
        let root = tempfile::TempDir::new().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        let destination = source.join("snapshot");
        assert_eq!(
            resolve_paths(&source, &destination).unwrap_err(),
            "destination must not be inside the source tree"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn small_tree_uses_copy_on_write_clonefile() {
        use std::os::unix::fs::symlink;

        let root = tempfile::TempDir::new().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file.txt"), "original").unwrap();
        symlink("file.txt", source.join("link.txt")).unwrap();
        let mut counts = CloneCounts::default();
        clone_tree(&source, &destination, &mut counts).unwrap();

        fs::write(destination.join("file.txt"), "changed").unwrap();
        assert_eq!(
            fs::read_to_string(source.join("file.txt")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_link(destination.join("link.txt")).unwrap(),
            Path::new("file.txt")
        );
        assert_eq!(counts.directories, 1);
        assert_eq!(counts.files, 1);
        assert_eq!(counts.links, 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn snapshot_skips_special_entries_only_in_git_storage() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let root = tempfile::TempDir::new().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(source.join(".git")).unwrap();
        let special = source.join(".git/fsmonitor--daemon.ipc");
        let special_c = CString::new(special.as_os_str().as_bytes()).unwrap();
        // SAFETY: special_c is a valid NUL-terminated path and names a new FIFO.
        assert_eq!(unsafe { libc::mkfifo(special_c.as_ptr(), 0o600) }, 0);
        let mut counts = CloneCounts::default();

        clone_tree(&source, &destination, &mut counts).unwrap();

        assert_eq!(counts.transient_git_entries_skipped, 1);
        assert!(!destination.join(".git/fsmonitor--daemon.ipc").exists());
    }
}

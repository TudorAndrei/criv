//! Confined lifecycle for content-addressed local State snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args as ClapArgs, Subcommand, ValueEnum};
use criv_state_wire::is_supported_schema;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::util::{remove_file_in, write_atomic_if_changed_in, write_atomic_in};
use crate::{CrivError, Result};

const STORE_DIR: &str = ".criv/snapshots";
const INDEX_PATH: &str = ".criv/snapshots/index.json";
const LATEST_PATH: &str = ".criv/latest";
const INDEX_SCHEMA: &str = "criv.snapshot-index.v0";

#[derive(Debug, ClapArgs)]
pub(crate) struct StateOptions {
    #[command(subcommand)]
    command: StateCommand,
}

#[derive(Debug, Subcommand)]
enum StateCommand {
    /// List retained local State snapshots newest first.
    List(ListOptions),
    /// Remove the oldest local State snapshots beyond a retention bound.
    Prune(PruneOptions),
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
enum Format {
    #[default]
    Text,
    Json,
}

#[derive(Debug, ClapArgs)]
struct ListOptions {
    /// Select deterministic text rows or a JSON array.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Debug, ClapArgs)]
struct PruneOptions {
    /// Override the configured number of snapshots to retain for this command.
    #[arg(long)]
    keep: Option<NonZeroUsize>,
    /// Report selected snapshots without changing local files.
    #[arg(long)]
    dry_run: bool,
    /// Select deterministic text rows or a JSON object.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct SnapshotRecord {
    hash: String,
    position: usize,
    bytes: u64,
    latest: bool,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct PruneReport {
    keep: usize,
    dry_run: bool,
    removed: Vec<SnapshotRecord>,
    retained: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct SnapshotIndex {
    schema: String,
    order: Vec<String>,
}

#[derive(Debug)]
struct SnapshotFile {
    bytes: u64,
    modified: SystemTime,
}

#[derive(Debug)]
struct StoreView {
    order: Vec<String>,
    files: BTreeMap<String, SnapshotFile>,
    latest: Option<String>,
}

pub(crate) fn run(root: &Path, options: StateOptions) -> Result<()> {
    match options.command {
        StateCommand::List(options) => print_list(&list(root)?, options.format),
        StateCommand::Prune(options) => {
            let keep = match options.keep {
                Some(keep) => keep.get(),
                None => Config::load(root)?.state_keep,
            };
            let report = prune(root, keep, options.dry_run)?;
            print_prune(&report, options.format)
        }
    }
}

pub(crate) fn publish(root: &Path, hash: &str, contents: &str, keep: usize) -> Result<()> {
    validate_keep(keep)?;
    validate_snapshot(hash, contents)?;
    let snapshot_path = snapshot_relative_path(hash);
    write_atomic_if_changed_in(root, Path::new(".criv"), &snapshot_path, contents)?;
    write_atomic_in(
        root,
        Path::new(".criv"),
        Path::new(LATEST_PATH),
        &format!("{hash}\n"),
    )?;
    let view = reconcile(root)?;
    apply_prune(root, view, keep, false)?;
    Ok(())
}

pub(crate) fn load(root: &Path, id: &str) -> Result<Option<String>> {
    let hash = if id == "latest" {
        read_latest(root)?
            .ok_or_else(|| CrivError::new("local snapshot `latest` does not resolve"))?
    } else if is_hash_reference(id) {
        id.to_string()
    } else {
        return Ok(None);
    };
    let Some(store) = existing_store_dir(root)? else {
        if id == "latest" {
            return Err(CrivError::new("local snapshot `latest` does not resolve"));
        }
        return Ok(None);
    };
    let path = store.join(format!("{hash}.json"));
    let Some(contents) = read_regular_file_optional(&path, "snapshot")? else {
        if id == "latest" {
            return Err(CrivError::new(format!(
                "local latest snapshot `{hash}` does not resolve"
            )));
        }
        return Ok(None);
    };
    validate_snapshot(&hash, &contents)?;
    Ok(Some(contents))
}

fn list(root: &Path) -> Result<Vec<SnapshotRecord>> {
    let view = reconcile(root)?;
    Ok(records_newest_first(&view))
}

fn prune(root: &Path, keep: usize, dry_run: bool) -> Result<PruneReport> {
    validate_keep(keep)?;
    let view = reconcile(root)?;
    apply_prune(root, view, keep, dry_run)
}

fn apply_prune(
    root: &Path,
    mut view: StoreView,
    keep: usize,
    dry_run: bool,
) -> Result<PruneReport> {
    let mut removed_hashes: Vec<String> = Vec::new();
    while view.order.len().saturating_sub(removed_hashes.len()) > keep {
        let next = view
            .order
            .iter()
            .find(|hash| {
                !removed_hashes.iter().any(|removed| removed == *hash)
                    && view.latest.as_ref() != Some(*hash)
            })
            .cloned();
        let Some(hash) = next else {
            break;
        };
        removed_hashes.push(hash);
    }

    let positions = view
        .order
        .iter()
        .rev()
        .enumerate()
        .map(|(index, hash)| (hash.clone(), index + 1))
        .collect::<BTreeMap<_, _>>();
    let removed = removed_hashes
        .iter()
        .map(|hash| SnapshotRecord {
            hash: hash.clone(),
            position: positions[hash],
            bytes: view.files[hash].bytes,
            latest: view.latest.as_ref() == Some(hash),
        })
        .collect::<Vec<_>>();
    let retained = view.order.len() - removed.len();

    if !dry_run {
        for hash in &removed_hashes {
            remove_file_in(root, Path::new(STORE_DIR), &snapshot_relative_path(hash))?;
            view.files.remove(hash);
        }
        let removed = removed_hashes.into_iter().collect::<BTreeSet<_>>();
        view.order.retain(|hash| !removed.contains(hash));
        if !view.order.is_empty() || existing_store_dir(root)?.is_some() {
            write_index(root, &view.order)?;
        }
    }

    Ok(PruneReport {
        keep,
        dry_run,
        retained,
        removed,
    })
}

fn reconcile(root: &Path) -> Result<StoreView> {
    let latest = read_latest(root)?;
    let Some(store) = existing_store_dir(root)? else {
        return Ok(StoreView {
            order: Vec::new(),
            files: BTreeMap::new(),
            latest,
        });
    };

    let mut files = BTreeMap::new();
    for entry in fs::read_dir(&store)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(hash) = name
            .strip_suffix(".json")
            .filter(|hash| is_managed_hash(hash))
        else {
            continue;
        };
        if entry.file_type()?.is_symlink() {
            return Err(CrivError::new(format!(
                "snapshot path {} must be a regular file",
                entry.path().display()
            )));
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            return Err(CrivError::new(format!(
                "snapshot path {} must be a regular file",
                entry.path().display()
            )));
        }
        let contents = fs::read_to_string(entry.path())
            .map_err(|err| CrivError::new(format!("failed to read snapshot `{hash}`: {err}")))?;
        validate_snapshot(hash, &contents)?;
        files.insert(
            hash.to_string(),
            SnapshotFile {
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            },
        );
    }

    let indexed = read_index(&store)?;
    let mut seen = BTreeSet::new();
    let mut order = indexed
        .unwrap_or_default()
        .into_iter()
        .filter(|hash| files.contains_key(hash) && seen.insert(hash.clone()))
        .collect::<Vec<_>>();
    let mut orphans = files
        .iter()
        .filter(|(hash, _)| !seen.contains(*hash))
        .map(|(hash, file)| (file.modified, hash.clone()))
        .collect::<Vec<_>>();
    orphans.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    order.extend(orphans.into_iter().map(|(_, hash)| hash));

    if let Some(latest_hash) = latest.as_ref().filter(|hash| files.contains_key(*hash)) {
        order.retain(|hash| hash != latest_hash);
        order.push(latest_hash.clone());
    }

    Ok(StoreView {
        order,
        files,
        latest,
    })
}

fn read_index(store: &Path) -> Result<Option<Vec<String>>> {
    let Some(contents) = read_regular_file_optional(&store.join("index.json"), "snapshot index")?
    else {
        return Ok(None);
    };
    let Ok(index) = serde_json::from_str::<SnapshotIndex>(&contents) else {
        return Ok(None);
    };
    if index.schema != INDEX_SCHEMA
        || index.order.iter().any(|hash| !is_managed_hash(hash))
        || index.order.iter().collect::<BTreeSet<_>>().len() != index.order.len()
    {
        return Ok(None);
    }
    Ok(Some(index.order))
}

fn read_latest(root: &Path) -> Result<Option<String>> {
    let Some(criv) = existing_criv_dir(root)? else {
        return Ok(None);
    };
    let path = criv.join("latest");
    let Some(contents) = read_regular_file_optional(&path, "latest snapshot pointer")? else {
        return Ok(None);
    };
    let hash = contents.trim();
    if !is_managed_hash(hash) {
        return Err(CrivError::new(format!(
            "latest snapshot pointer contains invalid hash `{hash}`"
        )));
    }
    Ok(Some(hash.to_string()))
}

fn write_index(root: &Path, order: &[String]) -> Result<()> {
    let json = serde_json::to_string_pretty(&SnapshotIndex {
        schema: INDEX_SCHEMA.into(),
        order: order.to_vec(),
    })
    .map_err(|err| CrivError::new(format!("failed to serialize snapshot index: {err}")))?;
    write_atomic_in(
        root,
        Path::new(STORE_DIR),
        Path::new(INDEX_PATH),
        &format!("{json}\n"),
    )
}

fn records_newest_first(view: &StoreView) -> Vec<SnapshotRecord> {
    view.order
        .iter()
        .rev()
        .enumerate()
        .map(|(index, hash)| SnapshotRecord {
            hash: hash.clone(),
            position: index + 1,
            bytes: view.files[hash].bytes,
            latest: view.latest.as_ref() == Some(hash),
        })
        .collect()
}

fn print_list(records: &[SnapshotRecord], format: Format) -> Result<()> {
    match format {
        Format::Text => {
            for record in records {
                println!(
                    "hash={} position={} bytes={} latest={}",
                    record.hash, record.position, record.bytes, record.latest
                );
            }
        }
        Format::Json => println!(
            "{}",
            serde_json::to_string(records)
                .map_err(|err| CrivError::new(format!("failed to serialize snapshots: {err}")))?
        ),
    }
    Ok(())
}

fn print_prune(report: &PruneReport, format: Format) -> Result<()> {
    match format {
        Format::Text => {
            for record in &report.removed {
                println!("remove hash={} bytes={}", record.hash, record.bytes);
            }
            println!(
                "prune keep={} removed={} retained={} dry_run={}",
                report.keep,
                report.removed.len(),
                report.retained,
                report.dry_run
            );
        }
        Format::Json => println!(
            "{}",
            serde_json::to_string(report).map_err(|err| {
                CrivError::new(format!("failed to serialize snapshot prune report: {err}"))
            })?
        ),
    }
    Ok(())
}

fn existing_store_dir(root: &Path) -> Result<Option<PathBuf>> {
    let Some(criv) = existing_criv_dir(root)? else {
        return Ok(None);
    };
    let store = criv.join("snapshots");
    let Some(_) = regular_directory_optional(&store, "snapshot store")? else {
        return Ok(None);
    };
    Ok(Some(store))
}

fn existing_criv_dir(root: &Path) -> Result<Option<PathBuf>> {
    let root = fs::canonicalize(root).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve vault root {} for snapshot access: {err}",
            root.display()
        ))
    })?;
    let criv = root.join(".criv");
    let Some(_) = regular_directory_optional(&criv, "snapshot state directory")? else {
        return Ok(None);
    };
    Ok(Some(criv))
}

fn regular_directory_optional(path: &Path, label: &str) -> Result<Option<()>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(CrivError::new(format!(
                "{label} {} must be a real directory",
                path.display()
            )))
        }
        Ok(_) => Ok(Some(())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn read_regular_file_optional(path: &Path, label: &str) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            CrivError::new(format!("{label} {} must be a regular file", path.display())),
        ),
        Ok(_) => fs::read_to_string(path).map(Some).map_err(Into::into),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn validate_snapshot(hash: &str, contents: &str) -> Result<()> {
    if !is_managed_hash(hash) {
        return Err(CrivError::new(format!("invalid snapshot hash `{hash}`")));
    }
    let value = serde_json::from_str::<serde_json::Value>(contents)
        .map_err(|err| CrivError::new(format!("snapshot `{hash}` is corrupt: {err}")))?;
    if !value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .is_some_and(is_supported_schema)
    {
        return Err(CrivError::new(format!(
            "snapshot `{hash}` is corrupt: expected schema {}",
            criv_state_wire::STATE_SCHEMA
        )));
    }
    let published = contents.strip_suffix('\n').unwrap_or(contents);
    let actual = blake3::hash(published.as_bytes()).to_hex().to_string();
    if actual != hash {
        return Err(CrivError::new(format!(
            "snapshot `{hash}` is corrupt: content hash is {actual}"
        )));
    }
    Ok(())
}

fn validate_keep(keep: usize) -> Result<()> {
    if keep == 0 {
        return Err(CrivError::new("snapshot keep must be a positive integer"));
    }
    Ok(())
}

fn snapshot_relative_path(hash: &str) -> PathBuf {
    Path::new(STORE_DIR).join(format!("{hash}.json"))
}

fn is_managed_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch))
}

fn is_hash_reference(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, FileTimes};
    use std::time::Duration;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use super::*;

    fn state(seed: &str) -> (String, String) {
        let contents =
            format!("{{\n  \"schema\": \"criv.state.v1\",\n  \"seed\": \"{seed}\"\n}}\n");
        let hash = blake3::hash(contents.trim_end_matches('\n').as_bytes())
            .to_hex()
            .to_string();
        (hash, contents)
    }

    #[test]
    fn publication_retains_unique_hashes_and_moves_repeated_hashes_newest() {
        let root = tempfile::TempDir::new().unwrap();
        let first = state("first");
        let second = state("second");
        let third = state("third");
        publish(root.path(), &first.0, &first.1, 2).unwrap();
        publish(root.path(), &second.0, &second.1, 2).unwrap();
        publish(root.path(), &first.0, &first.1, 2).unwrap();
        publish(root.path(), &third.0, &third.1, 2).unwrap();

        let records = list(root.path()).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| &record.hash)
                .collect::<Vec<_>>(),
            vec![&third.0, &first.0]
        );
        assert!(records[0].latest);
        assert!(!root.path().join(snapshot_relative_path(&second.0)).exists());
    }

    #[test]
    fn missing_or_corrupt_index_bootstraps_deterministically() {
        let root = tempfile::TempDir::new().unwrap();
        let first = state("first");
        let second = state("second");
        for item in [&second, &first] {
            write_atomic_in(
                root.path(),
                Path::new(".criv"),
                &snapshot_relative_path(&item.0),
                &item.1,
            )
            .unwrap();
            let file = File::options()
                .write(true)
                .open(root.path().join(snapshot_relative_path(&item.0)))
                .unwrap();
            file.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(10)))
                .unwrap();
        }
        write_atomic_in(
            root.path(),
            Path::new(STORE_DIR),
            Path::new(INDEX_PATH),
            "not json\n",
        )
        .unwrap();

        let records = list(root.path()).unwrap();
        let expected = if first.0 > second.0 {
            vec![first.0.clone(), second.0.clone()]
        } else {
            vec![second.0.clone(), first.0.clone()]
        };
        assert_eq!(
            records
                .into_iter()
                .map(|record| record.hash)
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn reconciliation_drops_missing_entries_and_adds_orphans() {
        let root = tempfile::TempDir::new().unwrap();
        let first = state("first");
        let orphan = state("orphan");
        for item in [&first, &orphan] {
            write_atomic_in(
                root.path(),
                Path::new(".criv"),
                &snapshot_relative_path(&item.0),
                &item.1,
            )
            .unwrap();
        }
        let missing = "f".repeat(64);
        write_index(root.path(), &[missing, first.0.clone()]).unwrap();
        write_atomic_in(
            root.path(),
            Path::new(".criv"),
            Path::new(LATEST_PATH),
            &format!("{}\n", orphan.0),
        )
        .unwrap();

        let records = list(root.path()).unwrap();
        assert_eq!(
            records
                .into_iter()
                .map(|record| record.hash)
                .collect::<Vec<_>>(),
            vec![orphan.0, first.0]
        );
    }

    #[test]
    fn dry_run_and_keep_override_protect_latest() {
        let root = tempfile::TempDir::new().unwrap();
        let first = state("first");
        let second = state("second");
        publish(root.path(), &first.0, &first.1, 20).unwrap();
        publish(root.path(), &second.0, &second.1, 20).unwrap();

        let preview = prune(root.path(), 1, true).unwrap();
        assert_eq!(preview.removed.len(), 1);
        assert!(root.path().join(snapshot_relative_path(&first.0)).exists());
        assert!(preview.removed.iter().all(|record| !record.latest));

        let applied = prune(root.path(), 1, false).unwrap();
        assert_eq!(applied.removed.len(), 1);
        assert_eq!(list(root.path()).unwrap()[0].hash, second.0);
    }

    #[test]
    fn corrupt_snapshots_fail_closed_and_are_preserved() {
        let root = tempfile::TempDir::new().unwrap();
        let corrupt_hash = "a".repeat(64);
        write_atomic_in(
            root.path(),
            Path::new(".criv"),
            &snapshot_relative_path(&corrupt_hash),
            "{}\n",
        )
        .unwrap();

        let error = prune(root.path(), 1, false).unwrap_err();
        assert!(error.to_string().contains("corrupt"));
        assert!(
            root.path()
                .join(snapshot_relative_path(&corrupt_hash))
                .exists()
        );
    }

    #[test]
    fn empty_store_lists_and_prunes_without_creating_state() {
        let root = tempfile::TempDir::new().unwrap();
        assert!(list(root.path()).unwrap().is_empty());
        assert_eq!(prune(root.path(), 1, false).unwrap().retained, 0);
        assert!(!root.path().join(".criv").exists());
    }

    #[test]
    fn local_lookup_does_not_claim_git_refs() {
        let root = tempfile::TempDir::new().unwrap();
        assert_eq!(load(root.path(), "HEAD").unwrap(), None);
        let item = state("one");
        publish(root.path(), &item.0, &item.1, 1).unwrap();
        assert_eq!(load(root.path(), "latest").unwrap(), Some(item.1));
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_access_rejects_symlinked_store() {
        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join(".criv")).unwrap();
        symlink(outside.path(), root.path().join(STORE_DIR)).unwrap();

        let error = list(root.path()).unwrap_err();
        assert!(error.to_string().contains("real directory"));
    }
}

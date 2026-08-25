//! Confined lifecycle for content-addressed local State snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::path::PathBuf;

use criv_state_wire::is_supported_schema;
use serde::{Deserialize, Serialize};

use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

#[cfg(test)]
const STORE_DIR: &str = ".criv/snapshots";
#[cfg(test)]
const INDEX_PATH: &str = ".criv/snapshots/index.json";
#[cfg(test)]
const LATEST_PATH: &str = ".criv/latest";
const INDEX_SCHEMA: &str = "criv.snapshot-index.v0";

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct SnapshotRecord {
    hash: String,
    position: usize,
    bytes: u64,
    latest: bool,
}

#[cfg(test)]
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
    #[cfg(test)]
    bytes: u64,
    modified: SystemTime,
}

#[derive(Debug)]
struct StoreView {
    order: Vec<String>,
    files: BTreeMap<String, SnapshotFile>,
    latest: Option<String>,
}

pub struct PublicationPlan {
    pub(crate) index_contents: String,
    pub(crate) removals: Vec<String>,
}

#[cfg(test)]
fn publish(root: &Path, hash: &str, contents: &str, keep: usize) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    plan_publication(&files, hash, contents, keep)?;
    publish_preflighted(&files, hash, contents, keep)
}

pub fn plan_publication(
    files: &RepositoryFiles,
    hash: &str,
    contents: &str,
    keep: usize,
) -> Result<PublicationPlan> {
    validate_keep(keep)?;
    validate_snapshot(hash, contents)?;
    let mut view = reconcile(files)?;
    if let Some(latest) = &view.latest
        && !view.files.contains_key(latest)
    {
        return Err(CrivError::new(format!(
            "local latest snapshot `{latest}` does not resolve"
        )));
    }
    view.order.retain(|existing| existing != hash);
    view.order.push(hash.to_string());
    let mut removals = Vec::new();
    while view.order.len().saturating_sub(removals.len()) > keep {
        let next = view
            .order
            .iter()
            .find(|candidate| {
                candidate.as_str() != hash && !removals.iter().any(|removed| removed == *candidate)
            })
            .cloned();
        let Some(next) = next else {
            break;
        };
        removals.push(next);
    }
    let removed = removals.iter().collect::<BTreeSet<_>>();
    let order = view
        .order
        .into_iter()
        .filter(|entry| !removed.contains(entry))
        .collect::<Vec<_>>();
    Ok(PublicationPlan {
        index_contents: index_contents(&order)?,
        removals,
    })
}

#[cfg(test)]
fn publish_preflighted(
    files: &RepositoryFiles,
    hash: &str,
    contents: &str,
    keep: usize,
) -> Result<()> {
    let mut view = reconcile(files)?;
    view.order.retain(|existing| existing != hash);
    view.order.push(hash.to_string());
    view.files.insert(
        hash.to_string(),
        SnapshotFile {
            #[cfg(test)]
            bytes: contents.len() as u64,
            modified: SystemTime::now(),
        },
    );
    view.latest = Some(hash.to_string());
    let snapshot_path = snapshot_relative_path(hash);
    let scope = files.write_scope(Path::new(".criv"))?;
    scope.write_atomic_if_changed(&snapshot_path, contents)?;
    scope.write_atomic(Path::new(LATEST_PATH), &format!("{hash}\n"))?;
    apply_prune(files, view, keep, false)?;
    Ok(())
}

pub fn load_unlocked(files: &RepositoryFiles, id: &str) -> Result<Option<String>> {
    let hash = if id == "latest" {
        read_latest(files)?
            .ok_or_else(|| CrivError::new("local snapshot `latest` does not resolve"))?
    } else if is_hash_reference(id) {
        id.to_string()
    } else {
        return Ok(None);
    };
    let Some(()) = existing_store_dir(files)? else {
        if id == "latest" {
            return Err(CrivError::new("local snapshot `latest` does not resolve"));
        }
        return Ok(None);
    };
    let path = format!(".criv/snapshots/{hash}.json");
    let Some(contents) = files.read_optional_string(Path::new(&path))? else {
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

#[cfg(test)]
fn list(root: &Path) -> Result<Vec<SnapshotRecord>> {
    let files = RepositoryFiles::open(root)?;
    let view = reconcile(&files)?;
    Ok(records_newest_first(&view))
}

#[cfg(test)]
fn prune(root: &Path, keep: usize, dry_run: bool) -> Result<PruneReport> {
    let files = RepositoryFiles::open(root)?;
    validate_keep(keep)?;
    let view = reconcile(&files)?;
    apply_prune(&files, view, keep, dry_run)
}

#[cfg(test)]
fn apply_prune(
    files: &RepositoryFiles,
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
            files
                .write_scope(Path::new(STORE_DIR))?
                .remove_file(&snapshot_relative_path(hash))?;
            view.files.remove(hash);
        }
        let removed = removed_hashes.into_iter().collect::<BTreeSet<_>>();
        view.order.retain(|hash| !removed.contains(hash));
        if !view.order.is_empty() || existing_store_dir(files)?.is_some() {
            write_index(files, &view.order)?;
        }
    }

    Ok(PruneReport {
        keep,
        dry_run,
        retained,
        removed,
    })
}

fn reconcile(files: &RepositoryFiles) -> Result<StoreView> {
    let latest = read_latest(files)?;
    let Some(()) = existing_store_dir(files)? else {
        return Ok(StoreView {
            order: Vec::new(),
            files: BTreeMap::new(),
            latest,
        });
    };

    let mut snapshot_files = BTreeMap::new();
    let names = files
        .read_dir_names(Path::new(".criv/snapshots"))?
        .unwrap_or_default();
    for name in names {
        let name = name.to_string_lossy().to_string();
        let Some(hash) = name
            .strip_suffix(".json")
            .filter(|hash| is_managed_hash(hash))
        else {
            continue;
        };
        let relative = format!(".criv/snapshots/{name}");
        let (contents, metadata) =
            files
                .read_with_metadata(Path::new(&relative))
                .map_err(|error| {
                    CrivError::new(format!("failed to read snapshot `{hash}`: {error}"))
                })?;
        let contents = String::from_utf8(contents).map_err(|error| {
            CrivError::new(format!("failed to read snapshot `{hash}`: {error}"))
        })?;
        validate_snapshot(hash, &contents)?;
        snapshot_files.insert(
            hash.to_string(),
            SnapshotFile {
                #[cfg(test)]
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            },
        );
    }

    let indexed = read_index(files)?;
    let mut seen = BTreeSet::new();
    let mut order = indexed
        .unwrap_or_default()
        .into_iter()
        .filter(|hash| snapshot_files.contains_key(hash) && seen.insert(hash.clone()))
        .collect::<Vec<_>>();
    let mut orphans = snapshot_files
        .iter()
        .filter(|(hash, _)| !seen.contains(*hash))
        .map(|(hash, file)| (file.modified, hash.clone()))
        .collect::<Vec<_>>();
    orphans.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    order.extend(orphans.into_iter().map(|(_, hash)| hash));

    if let Some(latest_hash) = latest
        .as_ref()
        .filter(|hash| snapshot_files.contains_key(*hash))
    {
        order.retain(|hash| hash != latest_hash);
        order.push(latest_hash.clone());
    }

    Ok(StoreView {
        order,
        files: snapshot_files,
        latest,
    })
}

fn read_index(files: &RepositoryFiles) -> Result<Option<Vec<String>>> {
    let Some(contents) = files.read_optional_string(Path::new(".criv/snapshots/index.json"))?
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

fn read_latest(files: &RepositoryFiles) -> Result<Option<String>> {
    let Some(contents) = files.read_optional_string(Path::new(".criv/latest"))? else {
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

#[cfg(test)]
fn write_index(files: &RepositoryFiles, order: &[String]) -> Result<()> {
    files
        .write_scope(Path::new(STORE_DIR))?
        .write_atomic(Path::new(INDEX_PATH), &index_contents(order)?)
}

fn index_contents(order: &[String]) -> Result<String> {
    let json = serde_json::to_string_pretty(&SnapshotIndex {
        schema: INDEX_SCHEMA.into(),
        order: order.to_vec(),
    })
    .map_err(|err| CrivError::new(format!("failed to serialize snapshot index: {err}")))?;
    Ok(format!("{json}\n"))
}

#[cfg(test)]
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

fn existing_store_dir(files: &RepositoryFiles) -> Result<Option<()>> {
    files
        .directory_exists(Path::new(".criv/snapshots"))
        .map(|exists| exists.then_some(()))
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

#[cfg(test)]
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
    use std::fs::{File, FileTimes};
    use std::time::Duration;

    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use super::*;

    fn write_fixture(
        root: &Path,
        allowed_dir: &Path,
        destination: &Path,
        contents: &str,
    ) -> Result<()> {
        RepositoryFiles::open(root)?
            .write_scope(allowed_dir)?
            .write_atomic(destination, contents)
    }

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
            write_fixture(
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
        write_fixture(
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
            write_fixture(
                root.path(),
                Path::new(".criv"),
                &snapshot_relative_path(&item.0),
                &item.1,
            )
            .unwrap();
        }
        let missing = "f".repeat(64);
        let files = RepositoryFiles::open(root.path()).unwrap();
        write_index(&files, &[missing, first.0.clone()]).unwrap();
        write_fixture(
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
        write_fixture(
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
        assert_eq!(
            crate::state::load_snapshot(root.path(), "HEAD").unwrap(),
            None
        );
        let item = state("one");
        publish(root.path(), &item.0, &item.1, 1).unwrap();
        assert_eq!(
            crate::state::load_snapshot(root.path(), "latest").unwrap(),
            Some(item.1)
        );
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_access_rejects_symlinked_store() {
        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join(".criv")).unwrap();
        symlink(outside.path(), root.path().join(STORE_DIR)).unwrap();

        let error = list(root.path()).unwrap_err();
        assert!(
            error.to_string().contains("symlinked vault path component"),
            "{error}"
        );
    }
}

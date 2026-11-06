//! The local snapshot store: reconciliation, retention planning, and lookup.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use criv_state_wire::{STATE_SCHEMA, content_hash, layout};
use serde::{Deserialize, Serialize};

use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

const INDEX_SCHEMA: &str = "criv.snapshot-index.v0";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SnapshotId {
    Latest,
    Hash(String),
}

impl std::str::FromStr for SnapshotId {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        if value == "latest" {
            Ok(Self::Latest)
        } else if content_hash::is_valid(value) {
            Ok(Self::Hash(value.to_string()))
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct SnapshotIndex {
    schema: String,
    order: Vec<String>,
}

#[derive(Debug)]
struct StoreView {
    order: Vec<String>,
    hashes: BTreeSet<String>,
    latest: Option<String>,
}

pub struct PublicationPlan {
    pub(crate) index_contents: String,
    pub(crate) removals: Vec<String>,
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
        && !view.hashes.contains(latest)
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

pub fn load_unlocked(files: &RepositoryFiles, id: &SnapshotId) -> Result<Option<String>> {
    let hash = match id {
        SnapshotId::Latest => read_latest(files)?
            .ok_or_else(|| CrivError::new("local snapshot `latest` does not resolve"))?,
        SnapshotId::Hash(hash) => hash.clone(),
    };
    if !store_exists(files)? {
        if *id == SnapshotId::Latest {
            return Err(CrivError::new("local snapshot `latest` does not resolve"));
        }
        return Ok(None);
    }
    let path = layout::snapshot(&hash);
    let Some(contents) = files.read_optional_string(Path::new(&path))? else {
        if *id == SnapshotId::Latest {
            return Err(CrivError::new(format!(
                "local latest snapshot `{hash}` does not resolve"
            )));
        }
        return Ok(None);
    };
    validate_snapshot(&hash, &contents)?;
    Ok(Some(contents))
}

/// Orders the valid stored snapshots oldest first: indexed hashes in index
/// order, then unindexed hashes by modification time and hash, then `latest`.
fn reconcile(files: &RepositoryFiles) -> Result<StoreView> {
    let latest = read_latest(files)?;
    let names = files
        .read_dir_names(Path::new(layout::SNAPSHOTS))?
        .unwrap_or_default();

    let mut modified = BTreeMap::new();
    for name in names {
        let name = name.to_string_lossy().to_string();
        let Some(hash) = name
            .strip_suffix(".json")
            .filter(|hash| content_hash::is_valid(hash))
        else {
            continue;
        };
        let relative = layout::snapshot(hash);
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
        modified.insert(hash.to_string(), metadata.modified().unwrap_or(UNIX_EPOCH));
    }

    let mut seen = BTreeSet::new();
    let mut order = read_index(files)?
        .unwrap_or_default()
        .into_iter()
        .filter(|hash| modified.contains_key(hash) && seen.insert(hash.clone()))
        .collect::<Vec<_>>();
    let mut orphans = modified
        .iter()
        .filter(|(hash, _)| !seen.contains(*hash))
        .map(|(hash, time)| (*time, hash.clone()))
        .collect::<Vec<(SystemTime, String)>>();
    orphans.sort();
    order.extend(orphans.into_iter().map(|(_, hash)| hash));

    if let Some(latest_hash) = latest.as_ref().filter(|hash| modified.contains_key(*hash)) {
        order.retain(|hash| hash != latest_hash);
        order.push(latest_hash.clone());
    }

    Ok(StoreView {
        order,
        hashes: modified.into_keys().collect(),
        latest,
    })
}

/// An unreadable or invalid index is repairable: it orders nothing, and the
/// next publication writes a fresh one (ADR-0094).
fn read_index(files: &RepositoryFiles) -> Result<Option<Vec<String>>> {
    let Some(contents) = files.read_optional_string(Path::new(layout::SNAPSHOT_INDEX))? else {
        return Ok(None);
    };
    let Ok(index) = serde_json::from_str::<SnapshotIndex>(&contents) else {
        return Ok(None);
    };
    if index.schema != INDEX_SCHEMA
        || index.order.iter().any(|hash| !content_hash::is_valid(hash))
        || index.order.iter().collect::<BTreeSet<_>>().len() != index.order.len()
    {
        return Ok(None);
    }
    Ok(Some(index.order))
}

fn read_latest(files: &RepositoryFiles) -> Result<Option<String>> {
    let Some(contents) = files.read_optional_string(Path::new(layout::LATEST))? else {
        return Ok(None);
    };
    let hash = contents.trim();
    if !content_hash::is_valid(hash) {
        return Err(CrivError::new(format!(
            "latest snapshot pointer contains invalid hash `{hash}`"
        )));
    }
    Ok(Some(hash.to_string()))
}

fn index_contents(order: &[String]) -> Result<String> {
    let json = serde_json::to_string_pretty(&SnapshotIndex {
        schema: INDEX_SCHEMA.into(),
        order: order.to_vec(),
    })
    .map_err(|err| CrivError::new(format!("failed to serialize snapshot index: {err}")))?;
    Ok(format!("{json}\n"))
}

fn store_exists(files: &RepositoryFiles) -> Result<bool> {
    files.directory_exists(Path::new(layout::SNAPSHOTS))
}

fn validate_snapshot(hash: &str, contents: &str) -> Result<()> {
    if !content_hash::is_valid(hash) {
        return Err(CrivError::new(format!("invalid snapshot hash `{hash}`")));
    }
    let value = serde_json::from_str::<serde_json::Value>(contents)
        .map_err(|err| CrivError::new(format!("snapshot `{hash}` is corrupt: {err}")))?;
    if value.get("schema").and_then(serde_json::Value::as_str) != Some(STATE_SCHEMA) {
        return Err(CrivError::new(format!(
            "snapshot `{hash}` is corrupt: expected schema {STATE_SCHEMA}"
        )));
    }
    let actual = content_hash::of_document(contents);
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
mod tests {
    use std::fs::{File, FileTimes};
    use std::time::Duration;

    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use super::*;

    fn write_fixture(root: &Path, allowed_dir: &str, destination: &str, contents: &str) {
        RepositoryFiles::open(root)
            .unwrap()
            .write_scope(Path::new(allowed_dir))
            .unwrap()
            .write_atomic(Path::new(destination), contents)
            .unwrap();
    }

    fn state(seed: &str) -> (String, String) {
        let contents =
            format!("{{\n  \"schema\": \"criv.state.v1\",\n  \"seed\": \"{seed}\"\n}}\n");
        (content_hash::of_document(&contents), contents)
    }

    fn order(root: &Path) -> Vec<String> {
        reconcile(&RepositoryFiles::open(root).unwrap())
            .unwrap()
            .order
    }

    #[test]
    fn missing_or_corrupt_index_orders_by_time_then_hash() {
        let root = tempfile::TempDir::new().unwrap();
        let first = state("first");
        let second = state("second");
        for item in [&second, &first] {
            write_fixture(
                root.path(),
                layout::DIR,
                &layout::snapshot(&item.0),
                &item.1,
            );
            File::options()
                .write(true)
                .open(root.path().join(layout::snapshot(&item.0)))
                .unwrap()
                .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(10)))
                .unwrap();
        }
        write_fixture(
            root.path(),
            layout::SNAPSHOTS,
            layout::SNAPSHOT_INDEX,
            "not json\n",
        );

        let mut expected = vec![first.0, second.0];
        expected.sort();
        assert_eq!(order(root.path()), expected);
    }

    #[test]
    fn reconciliation_drops_missing_entries_and_orders_latest_newest() {
        let root = tempfile::TempDir::new().unwrap();
        let first = state("first");
        let orphan = state("orphan");
        for item in [&first, &orphan] {
            write_fixture(
                root.path(),
                layout::DIR,
                &layout::snapshot(&item.0),
                &item.1,
            );
        }
        write_fixture(
            root.path(),
            layout::SNAPSHOTS,
            layout::SNAPSHOT_INDEX,
            &index_contents(&["f".repeat(64), first.0.clone()]).unwrap(),
        );
        write_fixture(
            root.path(),
            layout::DIR,
            layout::LATEST,
            &format!("{}\n", orphan.0),
        );

        assert_eq!(order(root.path()), vec![first.0, orphan.0]);
    }

    #[test]
    fn only_latest_and_full_hashes_name_local_snapshots() {
        let hash = "a".repeat(64);
        assert_eq!("latest".parse(), Ok(SnapshotId::Latest));
        assert_eq!(hash.parse(), Ok(SnapshotId::Hash(hash.clone())));
        for git_ref in ["HEAD", "HEAD~1", "abc123", "../../etc/passwd", ""] {
            assert!(git_ref.parse::<SnapshotId>().is_err(), "{git_ref}");
        }
        assert!(hash.to_uppercase().parse::<SnapshotId>().is_err());

        let root = tempfile::TempDir::new().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        assert_eq!(
            load_unlocked(&files, &SnapshotId::Hash(hash)).unwrap(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_access_rejects_symlinked_store() {
        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join(".criv")).unwrap();
        symlink(outside.path(), root.path().join(layout::SNAPSHOTS)).unwrap();

        let error = reconcile(&RepositoryFiles::open(root.path()).unwrap()).unwrap_err();
        assert!(
            error.to_string().contains("symlinked vault path component"),
            "{error}"
        );
    }
}

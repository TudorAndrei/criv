//! Reconciliation receipts and the proofs that read them.
//!
//! A receipt records the exact transaction a reconcile command produced. The
//! enforcement gate and the command use the same proofs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use criv_state_wire::{content_hash, layout};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::governs;
use crate::git::{ChangeStatus, ChangedEntry, GitRepository, Revision};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

/// The fields every receipt records about the run that wrote it.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ReceiptHeader {
    pub(super) schema: String,
    pub(super) base_ref: String,
    pub(super) head_sha: String,
    pub(super) target_sha: String,
    pub(super) merge_base: String,
}

/// A receipt file under `.criv/`.
pub(super) trait Receipt: DeserializeOwned + Serialize {
    const SCHEMA: &'static str;
    const PATH: &'static str;
    const NAME: &'static str;

    fn header(&self) -> &ReceiptHeader;

    /// The staged transaction is exactly this receipt.
    fn allows_transaction(&self, git: &GitRepository, entries: &[ChangedEntry]) -> bool;

    fn target_is_stable(&self, git: &GitRepository) -> bool {
        let header = self.header();
        target_is_stable(git, &header.base_ref, &header.target_sha)
    }

    fn read(files: &RepositoryFiles) -> Result<Option<Self>> {
        let Some(contents) = files.read_optional_string(Path::new(Self::PATH))? else {
            return Ok(None);
        };
        let receipt: Self = serde_json::from_str(&contents).map_err(|_| {
            CrivError::new(format!(
                "{} receipt is malformed; remove it only after recovering the worktree",
                Self::NAME
            ))
        })?;
        let schema = &receipt.header().schema;
        if schema != Self::SCHEMA {
            return Err(CrivError::new(format!(
                "{} receipt schema `{schema}` is unsupported; expected `{}`",
                Self::NAME,
                Self::SCHEMA
            )));
        }
        Ok(Some(receipt))
    }

    /// Whether this receipt was planned from the current `HEAD`. A receipt
    /// from an earlier `HEAD` is inert.
    fn planned_from_head(&self, git: &GitRepository) -> bool {
        git.resolve_commit("HEAD")
            .is_ok_and(|head| head == self.header().head_sha)
    }
}

/// Whether `base_ref` still resolves to the target the receipt was planned
/// against.
pub(super) fn target_is_stable(git: &GitRepository, base_ref: &str, target_sha: &str) -> bool {
    git.resolve_commit(base_ref)
        .is_ok_and(|sha| sha == target_sha)
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(super) struct SourceMapping {
    pub(super) old_path: String,
    pub(super) new_path: String,
}

/// The receipt of `criv adr reconcile-sources`.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SourceReceipt {
    #[serde(flatten)]
    pub(super) header: ReceiptHeader,
    pub(super) mappings: Vec<SourceMapping>,
    pub(super) files: Vec<SourceFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SourceFile {
    pub(super) path: String,
    pub(super) before_hash: String,
    pub(super) after_hash: String,
}

impl Receipt for SourceReceipt {
    const SCHEMA: &'static str = "criv.source-reconcile/1";
    const PATH: &'static str = layout::SOURCE_RECEIPT;
    const NAME: &'static str = "source reconciliation";

    fn header(&self) -> &ReceiptHeader {
        &self.header
    }

    /// The staged transaction rewrites exactly the receipt's ADRs, and each
    /// rewrite changes only the `governs:` paths of a proven rename.
    fn allows_transaction(&self, git: &GitRepository, entries: &[ChangedEntry]) -> bool {
        let renames_proven = git
            .changed_set(
                &crate::git::Comparison::Trees {
                    old_ref: &self.header.merge_base,
                    new_ref: &self.header.head_sha,
                },
                &[],
            )
            .and_then(|changes| rename_mappings(&changes.entries))
            .is_ok_and(|actual| {
                let actual = actual.into_iter().collect::<BTreeSet<_>>();
                self.mappings.iter().all(|mapping| actual.contains(mapping))
            });
        if !self.planned_from_head(git) || !self.target_is_stable(git) || !renames_proven {
            return false;
        }
        let expected = self
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<BTreeSet<_>>();
        let actual = entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<BTreeSet<_>>();
        let replacements = self
            .mappings
            .iter()
            .map(|mapping| (mapping.old_path.clone(), mapping.new_path.clone()))
            .collect();
        let head = Revision::Commit(self.header.head_sha.clone());
        expected == actual
            && entries.iter().all(|entry| {
                entry.status == ChangeStatus::Modified && entry.previous_path.is_none()
            })
            && self.files.iter().all(|file| {
                let (Ok(before), Ok(after)) = (
                    git.text(&head, &file.path),
                    git.text(&Revision::Index, &file.path),
                ) else {
                    return false;
                };
                content_hash::of_bytes(before.as_bytes()) == file.before_hash
                    && content_hash::of_bytes(after.as_bytes()) == file.after_hash
                    && governs::transition_matches(&before, &after, &replacements)
            })
    }
}

/// A modified decision whose only change follows `renames`, the Git renames
/// composed across the compared history.
pub(super) fn source_history_allows(
    git: &GitRepository,
    renames: &BTreeMap<String, String>,
    entry: &ChangedEntry,
) -> bool {
    if entry.status != ChangeStatus::Modified || entry.previous_path.is_some() || renames.is_empty()
    {
        return false;
    }
    let Some(old_ref) = entry.old_ref.as_deref() else {
        return false;
    };
    let old = git.text(&Revision::Commit(old_ref.to_owned()), &entry.path);
    let new = git.text(&entry.new, &entry.path);
    matches!((old, new), (Ok(old), Ok(new)) if governs::transition_matches(&old, &new, renames))
}

/// The renames among `entries` with each chain followed to its last path.
pub(super) fn composed_renames(entries: &[ChangedEntry]) -> Result<BTreeMap<String, String>> {
    let direct = rename_mappings(entries)?;
    let by_old = direct
        .iter()
        .map(|mapping| (mapping.old_path.as_str(), mapping.new_path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let destinations = direct
        .iter()
        .map(|mapping| mapping.new_path.as_str())
        .collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    for mapping in direct
        .iter()
        .filter(|mapping| !destinations.contains(mapping.old_path.as_str()))
    {
        let mut destination = mapping.new_path.as_str();
        let mut seen = BTreeSet::from([mapping.old_path.as_str()]);
        while let Some(next) = by_old.get(destination) {
            if !seen.insert(destination) {
                return Err(CrivError::new("cyclic Git rename mapping"));
            }
            destination = next;
        }
        result.insert(mapping.old_path.clone(), destination.to_string());
    }
    Ok(result)
}

/// The one-to-one renames among `entries`; copies are not renames.
pub(super) fn rename_mappings(entries: &[ChangedEntry]) -> Result<Vec<SourceMapping>> {
    let mut by_old = BTreeMap::new();
    let mut by_new = BTreeMap::new();
    for entry in entries {
        if entry.status != ChangeStatus::Renamed {
            continue;
        }
        let old_path = entry.previous_path.as_ref().ok_or_else(|| {
            CrivError::new(format!(
                "Git rename for `{}` has no source path",
                entry.path
            ))
        })?;
        if by_old
            .insert(old_path.clone(), entry.path.clone())
            .is_some()
            || by_new
                .insert(entry.path.clone(), old_path.clone())
                .is_some()
        {
            return Err(CrivError::new(format!(
                "ambiguous Git rename mapping involving `{old_path}` and `{}`",
                entry.path
            )));
        }
    }
    Ok(by_old
        .into_iter()
        .map(|(old_path, new_path)| SourceMapping { old_path, new_path })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(old: &str, new: &str) -> ChangedEntry {
        ChangedEntry {
            status: ChangeStatus::Renamed,
            path: new.into(),
            previous_path: Some(old.into()),
            old_ref: None,
            new: Revision::Index,
        }
    }

    #[test]
    fn recorded_receipts_round_trip_unchanged() {
        let (head, target, base) = ("1".repeat(40), "2".repeat(40), "3".repeat(40));
        let recorded = serde_json::json!({
            "schema": SourceReceipt::SCHEMA,
            "base_ref": "origin/main",
            "target_sha": target,
            "head_sha": head,
            "merge_base": base,
            "mappings": [{ "old_path": "src/old.rs", "new_path": "src/new.rs" }],
            "files": [{
                "path": "docs/adr/0001-base.md", "before_hash": "sha256:a", "after_hash": "sha256:b"
            }]
        });
        let receipt: SourceReceipt = serde_json::from_value(recorded.clone()).unwrap();
        assert_eq!(receipt.header().schema, SourceReceipt::SCHEMA);
        assert_eq!(serde_json::to_value(&receipt).unwrap(), recorded);
    }

    #[test]
    fn composes_chained_history_renames() {
        let entries = [
            rename("src/old.rs", "src/middle.rs"),
            rename("src/middle.rs", "src/new.rs"),
        ];
        assert_eq!(
            composed_renames(&entries).unwrap(),
            BTreeMap::from([("src/old.rs".to_owned(), "src/new.rs".to_owned())])
        );
    }

    #[test]
    fn excludes_copies_and_rejects_ambiguous_renames() {
        let mut copy = rename("src/old.rs", "src/copy.rs");
        copy.status = ChangeStatus::Copied;
        assert!(rename_mappings(&[copy]).unwrap().is_empty());
        assert!(
            rename_mappings(&[
                rename("src/old.rs", "src/one.rs"),
                rename("src/old.rs", "src/two.rs"),
            ])
            .is_err()
        );
    }
}

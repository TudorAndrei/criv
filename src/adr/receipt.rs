//! Reconciliation receipts and the proofs that read them.
//!
//! A receipt records the exact transaction a reconcile command produced. The
//! enforcement gate and the commands use the same proofs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use criv_state_wire::{content_hash, layout};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::governs;
use crate::git::{ChangeStatus, ChangedEntry, ChangedSet, GitRepository, Revision};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

pub(super) fn hash(bytes: &[u8]) -> String {
    content_hash::of_bytes(bytes)
}

/// A receipt file under `.criv/`.
pub(super) trait Receipt: DeserializeOwned + Serialize {
    const SCHEMA: &'static str;
    const PATH: &'static str;
    const NAME: &'static str;

    fn schema(&self) -> &str;
    fn head_sha(&self) -> &str;

    /// The receipt, or `None` when there is none.
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
        if receipt.schema() != Self::SCHEMA {
            return Err(CrivError::new(format!(
                "{} receipt schema `{}` is unsupported; expected `{}`",
                Self::NAME,
                receipt.schema(),
                Self::SCHEMA
            )));
        }
        Ok(Some(receipt))
    }

    /// Whether this receipt was planned from the current `HEAD`. A receipt
    /// from an earlier `HEAD` is inert.
    fn planned_from_head(&self, git: &GitRepository) -> bool {
        git.resolve_commit("HEAD")
            .is_ok_and(|head| head == self.head_sha())
    }
}

/// Whether `base_ref` still resolves to the target the receipt was planned
/// against.
pub(super) fn target_is_stable(git: &GitRepository, base_ref: &str, target_sha: &str) -> bool {
    git.resolve_commit(base_ref)
        .is_ok_and(|sha| sha == target_sha)
}

/// What a receipt records for one path at one revision.
struct Recorded<'a> {
    hash: &'a str,
    mode: &'a str,
}

/// Whether `revision` holds exactly what is recorded, where `None` records
/// absence. A failed read proves nothing.
fn holds(
    git: &GitRepository,
    revision: &Revision,
    path: &str,
    recorded: Option<Recorded<'_>>,
) -> bool {
    match (git.file(revision, path), recorded) {
        (Ok(None), None) => true,
        (Ok(Some(file)), Some(recorded)) => {
            hash(&file.bytes) == recorded.hash && file.mode == recorded.mode
        }
        _ => false,
    }
}

/// Text at `path` in `revision`, reading the worktree through the repository
/// handle.
pub(super) fn read_text(
    files: &RepositoryFiles,
    git: &GitRepository,
    revision: &Revision,
    path: &str,
) -> Result<String> {
    match revision {
        Revision::Worktree => files.read_string(Path::new(path)),
        revision => git.text(revision, path),
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct IdentityMapping {
    pub(super) old_id: u32,
    pub(super) new_id: u32,
    pub(super) old_path: String,
    pub(super) new_path: String,
}

/// The receipt of `criv adr reconcile`.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct IdentityReceipt {
    pub(super) schema: String,
    pub(super) base_ref: String,
    pub(super) head_sha: String,
    pub(super) target_sha: String,
    pub(super) merge_base: String,
    pub(super) mappings: Vec<IdentityMapping>,
    pub(super) sources: Vec<IdentitySource>,
    pub(super) deletions: Vec<String>,
    pub(super) files: Vec<IdentityFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct IdentityFile {
    pub(super) path: String,
    pub(super) before_hash: Option<String>,
    pub(super) after_hash: String,
    pub(super) before_mode: Option<String>,
    pub(super) after_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct IdentitySource {
    pub(super) path: String,
    pub(super) before_hash: String,
    pub(super) before_mode: String,
}

impl Receipt for IdentityReceipt {
    const SCHEMA: &'static str = "criv.adr-reconcile/3";
    const PATH: &'static str = layout::ADR_RECEIPT;
    const NAME: &'static str = "ADR reconciliation";

    fn schema(&self) -> &str {
        &self.schema
    }

    fn head_sha(&self) -> &str {
        &self.head_sha
    }
}

impl IdentityFile {
    /// Whether `revision` holds the recorded input; a file recorded without
    /// its mode, or a mode without its file, proves nothing.
    fn before_holds(&self, git: &GitRepository, revision: &Revision) -> bool {
        match (&self.before_hash, &self.before_mode) {
            (Some(hash), Some(mode)) => {
                holds(git, revision, &self.path, Some(Recorded { hash, mode }))
            }
            (None, None) => holds(git, revision, &self.path, None),
            _ => false,
        }
    }

    fn after(&self) -> Recorded<'_> {
        Recorded {
            hash: &self.after_hash,
            mode: &self.after_mode,
        }
    }
}

impl IdentityReceipt {
    /// Every path the transaction writes or deletes.
    pub(super) fn paths(&self) -> BTreeSet<&str> {
        self.files
            .iter()
            .map(|file| file.path.as_str())
            .chain(self.deletions.iter().map(String::as_str))
            .collect()
    }

    fn target_is_stable(&self, git: &GitRepository) -> bool {
        target_is_stable(git, &self.base_ref, &self.target_sha)
    }

    fn sources_hold(&self, git: &GitRepository) -> bool {
        let head = Revision::Commit(self.head_sha.clone());
        self.sources.iter().all(|source| {
            holds(
                git,
                &head,
                &source.path,
                Some(Recorded {
                    hash: &source.before_hash,
                    mode: &source.before_mode,
                }),
            )
        })
    }

    /// Whether `HEAD` held every input and `revision` holds every output.
    fn holds_at(&self, git: &GitRepository, revision: &Revision) -> bool {
        let head = Revision::Commit(self.head_sha.clone());
        self.files.iter().all(|file| {
            file.before_holds(git, &head) && holds(git, revision, &file.path, Some(file.after()))
        }) && self.deletions.iter().all(|path| {
            !self.files.iter().any(|file| file.path == *path) && holds(git, revision, path, None)
        })
    }

    fn paths_match(&self, entries: &[ChangedEntry]) -> bool {
        let actual = entries
            .iter()
            .flat_map(|entry| {
                std::iter::once(entry.path.as_str()).chain(entry.previous_path.as_deref())
            })
            .collect::<BTreeSet<_>>();
        actual == self.paths()
    }

    fn proves_index(&self, git: &GitRepository) -> bool {
        self.planned_from_head(git)
            && self.target_is_stable(git)
            && self.sources_hold(git)
            && self.holds_at(git, &Revision::Index)
    }

    /// The staged transaction is exactly this receipt.
    pub(super) fn allows_transaction(&self, git: &GitRepository, entries: &[ChangedEntry]) -> bool {
        self.proves_index(git) && self.paths_match(entries)
    }

    /// Git may present the staged ADR move as either a rename or a deletion.
    pub(super) fn allows_change(&self, git: &GitRepository, entry: &ChangedEntry) -> bool {
        if !self.proves_index(git) {
            return false;
        }
        match entry.status {
            ChangeStatus::Renamed => entry.previous_path.as_deref().is_some_and(|old_path| {
                self.mappings
                    .iter()
                    .any(|mapping| mapping.old_path == old_path && mapping.new_path == entry.path)
            }),
            ChangeStatus::Deleted => self.deletions.iter().any(|path| path == &entry.path),
            _ => false,
        }
    }

    /// `commit` is the one committed transaction after the planning `HEAD`.
    /// Push enforcement uses this after the checkout moved past that commit.
    pub(super) fn allows_commit(&self, git: &GitRepository, commit: &str) -> bool {
        if !matches!(git.first_parent(commit), Ok(Some(parent)) if parent == self.head_sha)
            || !self.target_is_stable(git)
            || !self.sources_hold(git)
        {
            return false;
        }
        git.changed_set(
            &crate::git::Comparison::Trees {
                old_ref: &self.head_sha,
                new_ref: commit,
            },
            &[],
        )
        .is_ok_and(|changes| {
            self.holds_at(git, &Revision::Commit(commit.to_owned()))
                && self.paths_match(&changes.entries)
        })
    }

    /// A combined push comparison may end at a later commit or merge. Admit
    /// only receipt paths whose exact transaction is in the compared history
    /// and whose outputs have not changed since that commit.
    pub(super) fn allows_history_change(
        &self,
        git: &GitRepository,
        changes: &ChangedSet,
        entry: &ChangedEntry,
    ) -> bool {
        let (Some(old_ref), Some(Revision::Commit(new_ref))) =
            (changes.old_ref.as_deref(), changes.new.as_ref())
        else {
            return false;
        };
        let Ok(commits) = git.commits_between(old_ref, new_ref) else {
            return false;
        };
        let expected = self.paths();
        commits.iter().any(|commit| self.allows_commit(git, commit))
            && self.outputs_survive(git, new_ref)
            && expected.contains(entry.path.as_str())
            && entry
                .previous_path
                .as_deref()
                .is_none_or(|path| expected.contains(path))
    }

    /// Outputs still hold at `tree`; a deleted path stays deleted or matches
    /// the target exactly.
    fn outputs_survive(&self, git: &GitRepository, tree: &str) -> bool {
        let tree = Revision::Commit(tree.to_owned());
        let target = Revision::Commit(self.target_sha.clone());
        self.files
            .iter()
            .all(|file| holds(git, &tree, &file.path, Some(file.after())))
            && self
                .deletions
                .iter()
                .all(|path| match git.file(&tree, path) {
                    Ok(None) => true,
                    Ok(Some(file)) => git
                        .file(&target, path)
                        .is_ok_and(|target| target == Some(file)),
                    Err(_) => false,
                })
    }

    /// The worktree holds exactly this receipt's transaction and nothing
    /// else, so a retry may re-plan it. The target ref is not checked: an
    /// advanced target is the reason to retry.
    pub(super) fn verify_materialized(&self, git: &GitRepository) -> Result<()> {
        if !self.planned_from_head(git)
            || !self.sources_hold(git)
            || !self.holds_at(git, &Revision::Worktree)
        {
            return Err(CrivError::new(
                "ADR reconciliation receipt does not match the materialized worktree",
            ));
        }
        let expected = self.paths();
        let actual = git.dirty_paths()?;
        let actual = actual.iter().map(String::as_str).collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(CrivError::new(format!(
                "ADR reconciliation receipt does not cover every dirty worktree path (expected: {}; actual: {})",
                expected.into_iter().collect::<Vec<_>>().join(", "),
                actual.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(super) struct SourceMapping {
    pub(super) old_path: String,
    pub(super) new_path: String,
}

/// The receipt of `criv adr reconcile-sources`.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SourceReceipt {
    pub(super) schema: String,
    pub(super) base_ref: String,
    pub(super) target_sha: String,
    pub(super) head_sha: String,
    pub(super) merge_base: String,
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

    fn schema(&self) -> &str {
        &self.schema
    }

    fn head_sha(&self) -> &str {
        &self.head_sha
    }
}

impl SourceReceipt {
    /// The staged transaction rewrites exactly the receipt's ADRs, and each
    /// rewrite changes only the `governs:` paths of a proven rename.
    pub(super) fn allows_transaction(&self, git: &GitRepository, entries: &[ChangedEntry]) -> bool {
        let renames_proven = git
            .changed_set(
                &crate::git::Comparison::Trees {
                    old_ref: &self.merge_base,
                    new_ref: &self.head_sha,
                },
                &[],
            )
            .and_then(|changes| rename_mappings(&changes.entries))
            .is_ok_and(|actual| {
                let actual = actual.into_iter().collect::<BTreeSet<_>>();
                self.mappings.iter().all(|mapping| actual.contains(mapping))
            });
        if !self.planned_from_head(git)
            || !target_is_stable(git, &self.base_ref, &self.target_sha)
            || !renames_proven
        {
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
        let head = Revision::Commit(self.head_sha.clone());
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
                hash(before.as_bytes()) == file.before_hash
                    && hash(after.as_bytes()) == file.after_hash
                    && governs::transition_matches(&before, &after, &replacements)
            })
    }
}

/// A modified decision whose only change follows the Git renames composed
/// across the compared history.
pub(super) fn source_history_allows(
    files: &RepositoryFiles,
    git: &GitRepository,
    changes: &ChangedSet,
    entry: &ChangedEntry,
) -> bool {
    if entry.status != ChangeStatus::Modified || entry.previous_path.is_some() {
        return false;
    }
    let Ok(replacements) = composed_renames(&changes.entries) else {
        return false;
    };
    let Some(old_ref) = entry.old_ref.as_deref() else {
        return false;
    };
    if replacements.is_empty() {
        return false;
    }
    let old = git.text(&Revision::Commit(old_ref.to_owned()), &entry.path);
    let new = read_text(files, git, &entry.new, &entry.path);
    matches!((old, new), (Ok(old), Ok(new)) if governs::transition_matches(&old, &new, &replacements))
}

fn composed_renames(entries: &[ChangedEntry]) -> Result<BTreeMap<String, String>> {
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

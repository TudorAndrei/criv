//! The embedded Git boundary for production repository access.
//!
//! Callers use criv values and errors only. `git2` objects stay in this module
//! so a runtime subprocess alternative cannot grow beside the embedded backend.

use std::path::Path;

use crate::{CrivError, Result};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ChangedSet {
    pub(crate) entries: Vec<ChangedEntry>,
    pub(crate) old_ref: Option<String>,
    pub(crate) new_ref: Option<String>,
    pub(crate) basis: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ChangedEntry {
    pub(crate) status: ChangeStatus,
    pub(crate) path: String,
    pub(crate) previous_path: Option<String>,
    pub(crate) old_ref: Option<String>,
    pub(crate) new_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Other,
}

/// A repository opened from an explicit vault root. Its `git2` handle remains
/// private so callers cannot depend on backend-specific values.
pub(crate) struct GitRepository {
    repository: git2::Repository,
}

pub(crate) enum ChangedSetComparison<'a> {
    Staged,
    Trees {
        old_ref: &'a str,
        new_ref: &'a str,
    },
    ThreeDot {
        upstream_ref: &'a str,
        head_ref: &'a str,
    },
    TreeToWorktree {
        old_ref: &'a str,
    },
}

impl GitRepository {
    /// Discovers the repository from `root`, treating bare repositories as not
    /// being inside a worktree to match `git rev-parse --is-inside-work-tree`.
    pub(crate) fn discover(root: &Path) -> Result<Option<Self>> {
        match git2::Repository::discover(root) {
            Ok(repository) if repository.workdir().is_some() => Ok(Some(Self { repository })),
            Ok(_) => Ok(None),
            Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(error) => Err(CrivError::new(format!(
                "failed to open repository from `{}`: {error}",
                root.display()
            ))),
        }
    }

    pub(crate) fn changed_set(&self, comparison: ChangedSetComparison<'_>) -> Result<ChangedSet> {
        match comparison {
            ChangedSetComparison::Staged => {
                let old_tree = self.head_tree();
                let diff = self
                    .repository
                    .diff_tree_to_index(old_tree.as_ref(), None, None)
                    .map_err(|error| {
                        CrivError::new(format!("git diff --cached failed: {error}"))
                    })?;
                self.changed_set_from_diff(diff, Some("HEAD"), Some(":"))
            }
            ChangedSetComparison::Trees { old_ref, new_ref } => {
                let old_tree = self.tree_at(old_ref).map_err(|error| {
                    CrivError::new(format!("git diff {old_ref} {new_ref} failed: {error}"))
                })?;
                let new_tree = self.tree_at(new_ref).map_err(|error| {
                    CrivError::new(format!("git diff {old_ref} {new_ref} failed: {error}"))
                })?;
                let diff = self
                    .repository
                    .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
                    .map_err(|error| {
                        CrivError::new(format!("git diff {old_ref} {new_ref} failed: {error}"))
                    })?;
                self.changed_set_from_diff(diff, Some(old_ref), Some(new_ref))
            }
            ChangedSetComparison::ThreeDot {
                upstream_ref,
                head_ref,
            } => {
                let upstream = self.commit_at(upstream_ref).map_err(|error| {
                    CrivError::new(format!(
                        "git diff {upstream_ref}...{head_ref} failed: {error}"
                    ))
                })?;
                let head = self.commit_at(head_ref).map_err(|error| {
                    CrivError::new(format!(
                        "git diff {upstream_ref}...{head_ref} failed: {error}"
                    ))
                })?;
                let merge_base = self
                    .repository
                    .merge_base(upstream.id(), head.id())
                    .map_err(|error| {
                        CrivError::new(format!(
                            "git diff {upstream_ref}...{head_ref} failed: {error}"
                        ))
                    })?;
                let base_tree = self
                    .repository
                    .find_commit(merge_base)
                    .and_then(|commit| commit.tree())
                    .map_err(|error| {
                        CrivError::new(format!(
                            "git diff {upstream_ref}...{head_ref} failed: {error}"
                        ))
                    })?;
                let head_tree = head.tree().map_err(|error| {
                    CrivError::new(format!(
                        "git diff {upstream_ref}...{head_ref} failed: {error}"
                    ))
                })?;
                let diff = self
                    .repository
                    .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)
                    .map_err(|error| {
                        CrivError::new(format!(
                            "git diff {upstream_ref}...{head_ref} failed: {error}"
                        ))
                    })?;
                self.changed_set_from_diff(diff, Some(upstream_ref), Some(head_ref))
            }
            ChangedSetComparison::TreeToWorktree { old_ref } => {
                let old_tree = self.tree_at(old_ref).map_err(|error| {
                    CrivError::new(format!("git diff {old_ref} failed: {error}"))
                })?;
                let diff = self
                    .repository
                    .diff_tree_to_workdir_with_index(Some(&old_tree), None)
                    .map_err(|error| {
                        CrivError::new(format!("git diff {old_ref} failed: {error}"))
                    })?;
                self.changed_set_from_diff(diff, Some(old_ref), None)
            }
        }
    }

    /// Returns outgoing commits in oldest-first order for the supported SHA-1
    /// pre-push input, matching `git rev-list --reverse` on the covered matrix.
    pub(crate) fn outgoing_commits(
        &self,
        remote_name: &str,
        local_oid: &str,
        remote_oid: &str,
    ) -> Result<Vec<String>> {
        let local_oid = parse_oid(local_oid, "local pre-push object ID")?;
        let mut walk = self
            .repository
            .revwalk()
            .map_err(|error| CrivError::new(format!("Git revision walk failed: {error}")))?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
            .map_err(|error| CrivError::new(format!("Git revision walk failed: {error}")))?;
        walk.push(local_oid)
            .map_err(|error| CrivError::new(format!("Git revision walk failed: {error}")))?;

        if is_zero_oid(remote_oid) {
            let prefix = format!("refs/remotes/{remote_name}/");
            for reference in self
                .repository
                .references()
                .map_err(|error| CrivError::new(format!("Git reference lookup failed: {error}")))?
            {
                let reference = reference.map_err(|error| {
                    CrivError::new(format!("Git reference lookup failed: {error}"))
                })?;
                if reference
                    .name()
                    .ok()
                    .is_some_and(|name| name.starts_with(&prefix))
                    && let Some(target) = reference.target()
                {
                    walk.hide(target).map_err(|error| {
                        CrivError::new(format!("Git revision walk failed: {error}"))
                    })?;
                }
            }
        } else {
            walk.hide(parse_oid(remote_oid, "remote pre-push object ID")?)
                .map_err(|error| CrivError::new(format!("Git revision walk failed: {error}")))?;
        }

        let mut commits = walk
            .map(|oid| {
                oid.map(|oid| oid.to_string())
                    .map_err(|error| CrivError::new(format!("Git revision walk failed: {error}")))
            })
            .collect::<Result<Vec<_>>>()?;
        commits.reverse();
        Ok(commits)
    }

    /// Returns one commit's changed entries, tagging each with its first
    /// parent (or no old ref for a root commit) and the commit itself.
    pub(crate) fn changed_set_for_commit(&self, commit_id: &str) -> Result<ChangedSet> {
        let commit = self
            .repository
            .find_commit(parse_oid(commit_id, "commit object ID")?)
            .map_err(|error| {
                CrivError::new(format!(
                    "Git commit `{commit_id}` does not resolve: {error}"
                ))
            })?;
        let old_ref = commit.parent_id(0).ok().map(|oid| oid.to_string());
        let old_tree = commit.parent(0).ok().and_then(|parent| parent.tree().ok());
        let new_tree = commit.tree().map_err(|error| {
            CrivError::new(format!("Git commit `{commit_id}` has no tree: {error}"))
        })?;
        let diff = self
            .repository
            .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)
            .map_err(|error| {
                CrivError::new(format!("Git commit diff `{commit_id}` failed: {error}"))
            })?;
        self.changed_set_from_diff(diff, old_ref.as_deref(), Some(commit_id))
    }

    pub(crate) fn read_file_at_ref(&self, reference: &str, path: &Path) -> Result<Vec<u8>> {
        let object = self
            .repository
            .revparse_single(reference)
            .map_err(|error| {
                CrivError::new(format!("git ref `{reference}` does not resolve: {error}"))
            })?;
        let tree = object.peel_to_tree().map_err(|error| {
            CrivError::new(format!(
                "git ref `{reference}` does not resolve to a tree: {error}"
            ))
        })?;
        let entry = tree.get_path(path).map_err(|error| {
            CrivError::new(format!(
                "git ref `{reference}` does not contain `{}`: {error}",
                path.display()
            ))
        })?;
        self.read_blob(entry.id(), reference, path)
    }

    pub(crate) fn read_file_at_index(&self, path: &Path) -> Result<Vec<u8>> {
        let index = self
            .repository
            .index()
            .map_err(|error| CrivError::new(format!("Git index could not be read: {error}")))?;
        let entry = index.get_path(path, 0).ok_or_else(|| {
            CrivError::new(format!("Git index does not contain `{}`", path.display()))
        })?;
        self.read_blob(entry.id, "index", path)
    }

    fn head_tree(&self) -> Option<git2::Tree<'_>> {
        self.repository
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok())
    }

    fn tree_at(&self, reference: &str) -> Result<git2::Tree<'_>> {
        self.repository
            .revparse_single(reference)
            .and_then(|object| object.peel_to_tree())
            .map_err(|error| {
                CrivError::new(format!(
                    "git ref `{reference}` does not resolve to a tree: {error}"
                ))
            })
    }

    fn commit_at(&self, reference: &str) -> Result<git2::Commit<'_>> {
        self.repository
            .revparse_single(reference)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| {
                CrivError::new(format!(
                    "git ref `{reference}` does not resolve to a commit: {error}"
                ))
            })
    }

    fn changed_set_from_diff(
        &self,
        mut diff: git2::Diff<'_>,
        old_ref: Option<&str>,
        new_ref: Option<&str>,
    ) -> Result<ChangedSet> {
        let mut similarity = git2::DiffFindOptions::new();
        similarity.renames(true).copies(true);
        diff.find_similar(Some(&mut similarity))
            .map_err(|error| CrivError::new(format!("git similarity detection failed: {error}")))?;

        let mut entries = Vec::new();
        for delta in diff.deltas() {
            let status = match delta.status() {
                git2::Delta::Added => ChangeStatus::Added,
                git2::Delta::Modified | git2::Delta::Typechange => ChangeStatus::Modified,
                git2::Delta::Deleted => ChangeStatus::Deleted,
                git2::Delta::Renamed => ChangeStatus::Renamed,
                git2::Delta::Copied => ChangeStatus::Copied,
                _ => ChangeStatus::Other,
            };
            let old_path = path_from_bytes(delta.old_file().path_bytes())?;
            let new_path = path_from_bytes(delta.new_file().path_bytes())?;
            let (path, previous_path) = match status {
                ChangeStatus::Deleted => (old_path, None),
                ChangeStatus::Renamed | ChangeStatus::Copied => (new_path, Some(old_path)),
                _ => (new_path, None),
            };
            entries.push(ChangedEntry {
                status,
                path,
                previous_path,
                old_ref: old_ref.map(str::to_string),
                new_ref: new_ref.map(str::to_string),
            });
        }

        Ok(ChangedSet {
            entries,
            old_ref: old_ref.map(str::to_string),
            new_ref: new_ref.map(str::to_string),
            basis: match (old_ref, new_ref) {
                (Some(old), Some(new)) => format!("Git comparison {old}..{new}"),
                (Some(old), None) => format!("Git comparison {old}..worktree"),
                (None, Some(new)) => format!("Git comparison ..{new}"),
                (None, None) => "Git comparison".into(),
            },
        })
    }

    fn read_blob(&self, oid: git2::Oid, source: &str, path: &Path) -> Result<Vec<u8>> {
        self.repository
            .find_blob(oid)
            .map(|blob| blob.content().to_vec())
            .map_err(|error| {
                CrivError::new(format!(
                    "Git {source} does not contain blob `{}`: {error}",
                    path.display()
                ))
            })
    }
}

fn path_from_bytes(path: Option<&[u8]>) -> Result<String> {
    let path = path.ok_or_else(|| CrivError::new("Git changed entry did not contain a path"))?;
    String::from_utf8(path.to_vec()).map_err(|_| {
        CrivError::new("Git changed path is not valid UTF-8; criv cannot represent it")
    })
}

fn parse_oid(value: &str, context: &str) -> Result<git2::Oid> {
    git2::Oid::from_str(value)
        .map_err(|error| CrivError::new(format!("invalid {context} `{value}`: {error}")))
}

fn is_zero_oid(oid: &str) -> bool {
    oid.bytes().all(|byte| byte == b'0')
}

/// Reads a file from the tree selected by `reference` in the repository
/// discovered from `root`.
pub(crate) fn read_file_at_ref(root: &Path, reference: &str, path: &Path) -> Result<Vec<u8>> {
    let repository = GitRepository::discover(root)?.ok_or_else(|| {
        CrivError::new(format!(
            "failed to open repository from `{}`",
            root.display()
        ))
    })?;
    repository.read_file_at_ref(reference, path)
}

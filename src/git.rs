//! The embedded Git boundary for production repository access.
//!
//! Callers use criv values and errors only. `git2` objects stay in this module
//! so a runtime subprocess alternative cannot grow beside the embedded backend.

use std::ops::Range;
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
                let mut options = similarity_diff_options();
                let diff = self
                    .repository
                    .diff_tree_to_index(old_tree.as_ref(), None, Some(&mut options))
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
                let mut options = similarity_diff_options();
                let diff = self
                    .repository
                    .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), Some(&mut options))
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
                let mut options = similarity_diff_options();
                let diff = self
                    .repository
                    .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut options))
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
                let mut options = similarity_diff_options();
                let diff = self
                    .repository
                    .diff_tree_to_workdir_with_index(Some(&old_tree), Some(&mut options))
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

    fn read_file_at_ref(&self, reference: &str, path: &Path) -> Result<Vec<u8>> {
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

    fn read_file_at_index(&self, path: &Path) -> Result<Vec<u8>> {
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
        similarity
            .renames(true)
            .copies(true)
            .copies_from_unmodified(true)
            .remove_unmodified(true)
            .rename_threshold(50)
            .copy_threshold(100);
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

fn similarity_diff_options() -> git2::DiffOptions {
    let mut options = git2::DiffOptions::new();
    options.include_unmodified(true);
    options
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

/// Resolves a reference to a commit ID without consulting inherited hook
/// environment. `Repository::discover` anchors all operations at `root`.
pub(crate) fn resolve_commit(root: &Path, reference: &str) -> Result<String> {
    let repository = required_repository(root)?;
    repository
        .commit_at(reference)
        .map(|commit| commit.id().to_string())
        .map_err(|_| CrivError::new(format!(
            "cannot resolve base ref `{reference}` to a commit; fetch complete history and retry"
        )))
}

pub(crate) fn is_repository(root: &Path) -> Result<bool> {
    Ok(GitRepository::discover(root)?.is_some())
}

pub(crate) fn changes_between(root: &Path, old: &str, new: &str) -> Result<ChangedSet> {
    changes_between_paths(root, old, new, &[])
}

pub(crate) fn changes_between_paths(
    root: &Path,
    old: &str,
    new: &str,
    paths: &[&str],
) -> Result<ChangedSet> {
    let repository = required_repository(root)?;
    let old_tree = repository.tree_at(old)?;
    let new_tree = repository.tree_at(new)?;
    let mut options = similarity_diff_options();
    for path in paths {
        options.pathspec(*path);
    }
    let diff = repository
        .repository
        .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), Some(&mut options))
        .map_err(|error| CrivError::new(format!("git diff {old} {new} failed: {error}")))?;
    repository.changed_set_from_diff(diff, Some(old), Some(new))
}

pub(crate) fn worktree_changes_in(root: &Path, paths: &[&str]) -> Result<ChangedSet> {
    let repository = required_repository(root)?;
    let old_tree = repository.tree_at("HEAD")?;
    let mut options = similarity_diff_options();
    for path in paths {
        options.pathspec(*path);
    }
    let diff = repository
        .repository
        .diff_tree_to_workdir_with_index(Some(&old_tree), Some(&mut options))
        .map_err(|error| CrivError::new(format!("git diff HEAD failed: {error}")))?;
    repository.changed_set_from_diff(diff, Some("HEAD"), None)
}

pub(crate) fn merge_base(root: &Path, first: &str, second: &str) -> Result<String> {
    let repository = required_repository(root)?;
    let first_ref = first;
    let second_ref = second;
    let first = repository.commit_at(first).map_err(|_| CrivError::new(format!(
        "cannot find a merge base for `{first_ref}` and `{second_ref}`; fetch complete history and retry"
    )))?;
    let second = repository.commit_at(second).map_err(|_| CrivError::new(format!(
        "cannot find a merge base for `{first_ref}` and `{second_ref}`; fetch complete history and retry"
    )))?;
    repository.repository.merge_base(first.id(), second.id())
        .map(|oid| oid.to_string())
        .map_err(|_| CrivError::new(format!(
            "cannot find a merge base for `{first_ref}` and `{second_ref}`; fetch complete history and retry"
        )))
}

pub(crate) fn tree_paths(root: &Path, reference: &str, prefix: &str) -> Result<Vec<String>> {
    let repository = required_repository(root)?;
    let tree = repository.tree_at(reference)?;
    let prefix = prefix.trim_start_matches("./").trim_end_matches('/');
    let mut paths = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |parent, entry| {
        let name = entry.name_bytes();
        let mut candidate = parent.as_bytes().to_vec();
        candidate.extend_from_slice(name);
        let Ok(candidate) = String::from_utf8(candidate) else {
            return -1;
        };
        if (prefix == "."
            || prefix.is_empty()
            || candidate == prefix
            || candidate.starts_with(&format!("{prefix}/")))
            && entry.kind() != Some(git2::ObjectType::Tree)
        {
            paths.push(candidate);
        }
        0
    })
    .map_err(|error| {
        CrivError::new(format!(
            "Git tree path is not valid UTF-8; criv cannot represent it: {error}"
        ))
    })?;
    Ok(paths)
}

pub(crate) fn blob(root: &Path, reference: &str, path: &str) -> Result<String> {
    let bytes = if reference == ":" {
        required_repository(root)?.read_file_at_index(Path::new(path))?
    } else {
        read_file_at_ref(root, reference, Path::new(path))?
    };
    String::from_utf8(bytes).map_err(|_| CrivError::new(format!(
        "Git blob `{reference}:{path}` is not valid UTF-8; reconciliation only supports text files"
    )))
}

pub(crate) fn file_mode(root: &Path, reference: &str, path: &str) -> Result<Option<String>> {
    let repository = required_repository(root)?;
    let mode = if reference == ":" {
        repository
            .repository
            .index()
            .map_err(|error| CrivError::new(format!("Git index could not be read: {error}")))?
            .get_path(Path::new(path), 0)
            .map(|entry| entry.mode)
    } else {
        repository
            .tree_at(reference)?
            .get_path(Path::new(path))
            .ok()
            .map(|entry| entry.filemode() as u32)
    };
    Ok(mode.map(|mode| format!("{mode:06o}")))
}

pub(crate) fn added_lines(
    root: &Path,
    old: &str,
    new: &str,
    path: &str,
) -> Result<Vec<Range<usize>>> {
    let repository = required_repository(root)?;
    let old_tree = repository.tree_at(old)?;
    let new_tree = repository.tree_at(new)?;
    let old_blob = old_tree
        .get_path(Path::new(path))
        .ok()
        .and_then(|entry| repository.repository.find_blob(entry.id()).ok());
    let new_blob = new_tree
        .get_path(Path::new(path))
        .ok()
        .and_then(|entry| repository.repository.find_blob(entry.id()).ok());
    added_lines_for_blobs(
        &repository.repository,
        old_blob.as_ref(),
        path,
        new_blob.as_ref(),
        path,
    )
}

pub(crate) fn added_lines_between_blobs(
    root: &Path,
    old: &str,
    old_path: &str,
    new: &str,
    new_path: &str,
) -> Result<Vec<Range<usize>>> {
    let repository = required_repository(root)?;
    let old_blob = repository
        .tree_at(old)?
        .get_path(Path::new(old_path))
        .ok()
        .and_then(|entry| repository.repository.find_blob(entry.id()).ok());
    let new_blob = repository
        .tree_at(new)?
        .get_path(Path::new(new_path))
        .ok()
        .and_then(|entry| repository.repository.find_blob(entry.id()).ok());
    added_lines_for_blobs(
        &repository.repository,
        old_blob.as_ref(),
        old_path,
        new_blob.as_ref(),
        new_path,
    )
}

fn added_lines_for_blobs(
    repository: &git2::Repository,
    old: Option<&git2::Blob<'_>>,
    old_path: &str,
    new: Option<&git2::Blob<'_>>,
    new_path: &str,
) -> Result<Vec<Range<usize>>> {
    let mut ranges = Vec::new();
    let mut line_callback =
        |_: git2::DiffDelta<'_>, _: Option<git2::DiffHunk<'_>>, line: git2::DiffLine<'_>| {
            if line.origin() == '+' && line.new_lineno().is_some_and(|line_no| line_no > 0) {
                let line_no = line.new_lineno().unwrap() as usize;
                ranges.push(line_no..line_no + 1);
            }
            true
        };
    repository
        .diff_blobs(
            old,
            Some(old_path),
            new,
            Some(new_path),
            None,
            None,
            None,
            None,
            Some(&mut line_callback),
        )
        .map_err(|error| {
            CrivError::new(format!("cannot diff Git blobs for `{new_path}`: {error}"))
        })?;
    Ok(coalesce_ranges(ranges))
}

fn coalesce_ranges(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut result: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = result.last_mut()
            && previous.end == range.start
        {
            previous.end = range.end;
        } else {
            result.push(range);
        }
    }
    result
}

pub(crate) fn dirty_paths(root: &Path) -> Result<Vec<String>> {
    let repository = required_repository(root)?;
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repository
        .repository
        .statuses(Some(&mut options))
        .map_err(|error| CrivError::new(format!("Git status failed: {error}")))?;
    let mut paths = Vec::new();
    for status in statuses.iter() {
        if let Ok(path) = status.path() {
            paths.push(path.to_string());
        }
        if let Some(path) = status
            .head_to_index()
            .and_then(|delta| delta.old_file().path())
        {
            paths.push(path.to_string_lossy().to_string());
        }
        if let Some(path) = status
            .head_to_index()
            .and_then(|delta| delta.new_file().path())
        {
            paths.push(path.to_string_lossy().to_string());
        }
        if let Some(path) = status
            .index_to_workdir()
            .and_then(|delta| delta.old_file().path())
        {
            paths.push(path.to_string_lossy().to_string());
        }
        if let Some(path) = status
            .index_to_workdir()
            .and_then(|delta| delta.new_file().path())
        {
            paths.push(path.to_string_lossy().to_string());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(crate) fn reset_index_paths(root: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let repository = required_repository(root)?;
    let target = repository
        .repository
        .head()
        .and_then(|head| head.peel_to_commit())
        .map(|commit| commit.into_object())
        .map_err(|error| CrivError::new(format!("Git HEAD could not be read: {error}")))?;
    repository
        .repository
        .reset_default(Some(&target), paths.iter().map(Path::new))
        .map_err(|error| CrivError::new(format!("Git index reset failed: {error}")))
}

pub(crate) fn ref_is_stable(root: &Path, reference: &str, expected: &str) -> Result<bool> {
    Ok(resolve_commit(root, reference)? == expected)
}

pub(crate) fn first_parent(root: &Path, commit: &str) -> Result<Option<String>> {
    let repository = required_repository(root)?;
    let commit = repository.commit_at(commit)?;
    Ok(commit.parent_id(0).ok().map(|oid| oid.to_string()))
}

fn required_repository(root: &Path) -> Result<GitRepository> {
    GitRepository::discover(root)?.ok_or_else(|| {
        CrivError::new(format!(
            "failed to open repository from `{}`",
            root.display()
        ))
    })
}

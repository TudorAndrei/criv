//! The embedded Git boundary for production repository access.
//!
//! Callers use criv values and errors only. `git2` objects stay in this module
//! so a runtime subprocess alternative cannot grow beside the embedded backend.

use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;
use std::path::Path;

use crate::diagnostic::DiagnosticCode;
use crate::{CrivError, Result};

/// One side of a Git comparison.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Revision {
    /// A reference or object ID that peels to a commit tree.
    Commit(String),
    /// The staged index.
    Index,
    /// The worktree, filtered as Git would stage it.
    Worktree,
}

impl fmt::Display for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commit(reference) => formatter.write_str(reference),
            Self::Index => formatter.write_str(":"),
            Self::Worktree => formatter.write_str("worktree"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChangedSet {
    pub(crate) entries: Vec<ChangedEntry>,
    pub(crate) old_ref: Option<String>,
    /// The compared revision, absent when the entries come from several
    /// commits, as for pre-push input.
    pub(crate) new: Option<Revision>,
    pub(crate) basis: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChangedEntry {
    pub(crate) status: ChangeStatus,
    pub(crate) path: String,
    pub(crate) previous_path: Option<String>,
    /// The commit the entry is compared from, absent for a root commit.
    pub(crate) old_ref: Option<String>,
    pub(crate) new: Revision,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Other,
}

impl ChangedSet {
    /// Every repository-relative path whose identity participates in this
    /// transaction. Renames and copies include both sides so consumers cannot
    /// accidentally ignore an old path that stopped existing.
    pub(crate) fn affected_paths(&self) -> Vec<String> {
        let mut paths = BTreeSet::new();
        for entry in &self.entries {
            paths.insert(entry.path.clone());
            if let Some(previous) = &entry.previous_path {
                paths.insert(previous.clone());
            }
        }
        paths.into_iter().collect()
    }
}

/// The selection of a changed set.
pub enum Comparison<'a> {
    /// `HEAD` against the index.
    Staged,
    /// One commit tree against another.
    Trees { old_ref: &'a str, new_ref: &'a str },
    /// The merge base of `upstream_ref` and `HEAD` against `HEAD`.
    SinceMergeBase { upstream_ref: &'a str },
    /// A commit tree against the worktree.
    Worktree { old_ref: &'a str },
}

impl fmt::Display for Comparison<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Staged => formatter.write_str("git diff --cached"),
            Self::Trees { old_ref, new_ref } => write!(formatter, "git diff {old_ref} {new_ref}"),
            Self::SinceMergeBase { upstream_ref } => {
                write!(formatter, "git diff {upstream_ref}...HEAD")
            }
            Self::Worktree { old_ref } => write!(formatter, "git diff {old_ref}"),
        }
    }
}

/// A file as one revision holds it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GitFile {
    pub(crate) bytes: Vec<u8>,
    /// The Git file mode in octal, such as `100644`.
    pub(crate) mode: String,
}

/// A repository opened from an explicit vault root. Its `git2` handle remains
/// private so callers cannot depend on backend-specific values.
pub struct GitRepository {
    repository: git2::Repository,
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn git_error(context: impl fmt::Display) -> impl FnOnce(git2::Error) -> CrivError {
    move |error| CrivError::new(format!("{context}: {error}"))
}

impl GitRepository {
    /// Discovers the repository from `root`, treating bare repositories as not
    /// being inside a worktree to match `git rev-parse --is-inside-work-tree`.
    pub(crate) fn discover(root: &Path) -> Result<Option<Self>> {
        match git2::Repository::discover(root) {
            Ok(repository) => {
                let Some(workdir) = repository.workdir().map(Path::to_path_buf) else {
                    return Ok(None);
                };
                if !same_directory(&workdir, root) {
                    return Err(CrivError::coded(
                        DiagnosticCode::VaultOutsideGitRoot,
                        format!(
                            "the vault at `{}` is not the root of the Git worktree at `{}`; every path basis would shift",
                            root.display(),
                            workdir.display()
                        ),
                    ));
                }
                Ok(Some(Self { repository }))
            }
            Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(error) => Err(CrivError::new(format!(
                "failed to open repository from `{}`: {error}",
                root.display()
            ))),
        }
    }

    /// Opens the worktree repository rooted at `root`, which must exist.
    pub(crate) fn open(root: &Path) -> Result<Self> {
        Self::discover(root)?.ok_or_else(|| {
            CrivError::new(format!(
                "failed to open repository from `{}`",
                root.display()
            ))
        })
    }

    /// The repository's Git directory, such as `.git`.
    pub(crate) fn git_dir(&self) -> &Path {
        self.repository.path()
    }

    /// Lists changed entries, restricted to `paths` when any are given.
    pub(crate) fn changed_set(
        &self,
        comparison: &Comparison<'_>,
        paths: &[&str],
    ) -> Result<ChangedSet> {
        let failed = |error: CrivError| CrivError::new(format!("{comparison} failed: {error}"));
        let mut options = git2::DiffOptions::new();
        options.include_unmodified(true);
        for path in paths {
            options.pathspec(*path);
        }
        let (diff, old_ref, new) = match comparison {
            Comparison::Staged => {
                let old_tree = self.head_tree();
                let diff =
                    self.repository
                        .diff_tree_to_index(old_tree.as_ref(), None, Some(&mut options));
                (diff, "HEAD".to_owned(), Revision::Index)
            }
            Comparison::Trees { old_ref, new_ref } => {
                let old_tree = self.tree_at(old_ref).map_err(failed)?;
                let new_tree = self.tree_at(new_ref).map_err(failed)?;
                let diff = self.repository.diff_tree_to_tree(
                    Some(&old_tree),
                    Some(&new_tree),
                    Some(&mut options),
                );
                (
                    diff,
                    (*old_ref).to_owned(),
                    Revision::Commit((*new_ref).to_owned()),
                )
            }
            Comparison::SinceMergeBase { upstream_ref } => {
                let merge_base = self.merge_base(upstream_ref, "HEAD").map_err(failed)?;
                let base_tree = self.tree_at(&merge_base).map_err(failed)?;
                let head_tree = self.tree_at("HEAD").map_err(failed)?;
                let diff = self.repository.diff_tree_to_tree(
                    Some(&base_tree),
                    Some(&head_tree),
                    Some(&mut options),
                );
                (
                    diff,
                    (*upstream_ref).to_owned(),
                    Revision::Commit("HEAD".to_owned()),
                )
            }
            Comparison::Worktree { old_ref } => {
                let old_tree = self.tree_at(old_ref).map_err(failed)?;
                let diff = self
                    .repository
                    .diff_tree_to_workdir_with_index(Some(&old_tree), Some(&mut options));
                (diff, (*old_ref).to_owned(), Revision::Worktree)
            }
        };
        let diff = diff.map_err(|error| CrivError::new(format!("{comparison} failed: {error}")))?;
        let entries = changed_entries(diff, Some(&old_ref), &new)?;
        Ok(ChangedSet {
            entries,
            basis: format!("Git comparison {old_ref}..{new}"),
            old_ref: Some(old_ref),
            new: Some(new),
        })
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
        let mut hidden = Vec::new();
        if is_zero_oid(remote_oid) {
            let prefix = format!("refs/remotes/{remote_name}/");
            for reference in self
                .repository
                .references()
                .map_err(git_error("Git reference lookup failed"))?
            {
                let reference = reference.map_err(git_error("Git reference lookup failed"))?;
                if reference.name().is_ok_and(|name| name.starts_with(&prefix))
                    && let Some(target) = reference.target()
                {
                    hidden.push(target);
                }
            }
        } else {
            hidden.push(parse_oid(remote_oid, "remote pre-push object ID")?);
        }
        self.walk(local_oid, &hidden)
    }

    /// Returns one commit's changed entries, tagging each with its first
    /// parent (or no old ref for a root commit) and the commit itself.
    pub(crate) fn changed_set_for_commit(&self, commit_id: &str) -> Result<ChangedSet> {
        let commit = self
            .repository
            .find_commit(parse_oid(commit_id, "commit object ID")?)
            .map_err(git_error(format!(
                "Git commit `{commit_id}` does not resolve"
            )))?;
        let old_ref = commit.parent_id(0).ok().map(|oid| oid.to_string());
        let old_tree = commit.parent(0).ok().and_then(|parent| parent.tree().ok());
        let new_tree = commit
            .tree()
            .map_err(git_error(format!("Git commit `{commit_id}` has no tree")))?;
        let diff = self
            .repository
            .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)
            .map_err(git_error(format!("Git commit diff `{commit_id}` failed")))?;
        let new = Revision::Commit(commit_id.to_owned());
        let entries = changed_entries(diff, old_ref.as_deref(), &new)?;
        Ok(ChangedSet {
            entries,
            basis: format!(
                "Git comparison {}..{commit_id}",
                old_ref.as_deref().unwrap_or_default()
            ),
            old_ref,
            new: Some(new),
        })
    }

    /// Commits reachable from `new_ref` and not from `old_ref`, oldest first.
    pub(crate) fn commits_between(&self, old_ref: &str, new_ref: &str) -> Result<Vec<String>> {
        let old = self.commit_at(old_ref)?;
        let new = self.commit_at(new_ref)?;
        self.walk(new.id(), &[old.id()])
    }

    fn walk(&self, start: git2::Oid, hidden: &[git2::Oid]) -> Result<Vec<String>> {
        let failed = || git_error("Git revision walk failed");
        let mut walk = self.repository.revwalk().map_err(failed())?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
            .map_err(failed())?;
        walk.push(start).map_err(failed())?;
        for oid in hidden {
            walk.hide(*oid).map_err(failed())?;
        }
        let mut commits = walk
            .map(|oid| oid.map(|oid| oid.to_string()).map_err(failed()))
            .collect::<Result<Vec<_>>>()?;
        commits.reverse();
        Ok(commits)
    }

    /// Resolves a reference to a commit ID without consulting inherited hook
    /// environment.
    pub(crate) fn resolve_commit(&self, reference: &str) -> Result<String> {
        self.commit_at(reference)
            .map(|commit| commit.id().to_string())
    }

    pub(crate) fn first_parent(&self, commit: &str) -> Result<Option<String>> {
        Ok(self
            .commit_at(commit)?
            .parent_id(0)
            .ok()
            .map(|oid| oid.to_string()))
    }

    pub(crate) fn merge_base(&self, first: &str, second: &str) -> Result<String> {
        let first_commit = self.commit_at(first)?;
        let second_commit = self.commit_at(second)?;
        self.repository
            .merge_base(first_commit.id(), second_commit.id())
            .map(|oid| oid.to_string())
            .map_err(git_error(format!(
                "Git found no merge base for `{first}` and `{second}`"
            )))
    }

    /// Every file path at `reference` below `prefix`; `.` or an empty prefix
    /// lists the whole tree.
    pub(crate) fn tree_paths(&self, reference: &str, prefix: &str) -> Result<Vec<String>> {
        let tree = self.tree_at(reference)?;
        let prefix = prefix.trim_start_matches("./").trim_end_matches('/');
        let mut paths = Vec::new();
        tree.walk(git2::TreeWalkMode::PreOrder, |parent, entry| {
            let mut candidate = parent.as_bytes().to_vec();
            candidate.extend_from_slice(entry.name_bytes());
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
        .map_err(git_error(
            "Git tree path is not valid UTF-8; criv cannot represent it",
        ))?;
        Ok(paths)
    }

    /// The file at `path` in `revision`, or `None` when the revision has no
    /// file there. A worktree file is read as Git would stage it, including
    /// clean filters such as `core.autocrlf`, without writing the index.
    pub(crate) fn file(&self, revision: &Revision, path: &str) -> Result<Option<GitFile>> {
        let relative = Path::new(path);
        let entry = match revision {
            Revision::Commit(reference) => {
                let entry = match self.tree_at(reference)?.get_path(relative) {
                    Ok(entry) => entry,
                    Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
                    Err(error) => {
                        return Err(CrivError::new(format!(
                            "git ref `{reference}` cannot read `{path}`: {error}"
                        )));
                    }
                };
                let mode = u32::try_from(entry.filemode())
                    .map_err(|_| CrivError::new("Git tree entry has a negative file mode"))?;
                (entry.id(), mode)
            }
            Revision::Index => match self.index()?.get_path(relative, 0) {
                Some(entry) => (entry.id, entry.mode),
                None => return Ok(None),
            },
            Revision::Worktree => {
                let mut index = self.index()?;
                match index.add_path(relative) {
                    Ok(()) => {}
                    Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
                    Err(error) => {
                        return Err(CrivError::new(format!(
                            "Git worktree file `{path}` could not be filtered for staging: {error}"
                        )));
                    }
                }
                let entry = index.get_path(relative, 0).ok_or_else(|| {
                    CrivError::new(format!(
                        "Git worktree file `{path}` did not produce an index entry"
                    ))
                })?;
                (entry.id, entry.mode)
            }
        };
        let bytes = self
            .repository
            .find_blob(entry.0)
            .map(|blob| blob.content().to_vec())
            .map_err(git_error(format!(
                "Git {revision} does not contain blob `{path}`"
            )))?;
        Ok(Some(GitFile {
            bytes,
            mode: format!("{:06o}", entry.1),
        }))
    }

    /// The UTF-8 text at `path` in `revision`, which must exist.
    pub(crate) fn text(&self, revision: &Revision, path: &str) -> Result<String> {
        let file = self
            .file(revision, path)?
            .ok_or_else(|| CrivError::new(format!("Git {revision} does not contain `{path}`")))?;
        String::from_utf8(file.bytes)
            .map_err(|_| CrivError::new(format!("Git blob `{revision}:{path}` is not valid UTF-8")))
    }

    /// Line ranges of `new_path` at `new_ref` that are additions relative to
    /// `old_path` at `old_ref`; a missing side compares as empty.
    pub(crate) fn added_lines(
        &self,
        old_ref: &str,
        old_path: &str,
        new_ref: &str,
        new_path: &str,
    ) -> Result<Vec<Range<usize>>> {
        let old_blob = self.tree_blob(old_ref, old_path)?;
        let new_blob = self.tree_blob(new_ref, new_path)?;
        let mut ranges = Vec::new();
        let mut line_callback =
            |_: git2::DiffDelta<'_>, _: Option<git2::DiffHunk<'_>>, line: git2::DiffLine<'_>| {
                if let Some(line_no) = line.new_lineno().filter(|_| line.origin() == '+')
                    && let Ok(start) = usize::try_from(line_no)
                    && let Some(end) = start.checked_add(1)
                {
                    ranges.push(start..end);
                }
                true
            };
        self.repository
            .diff_blobs(
                old_blob.as_ref(),
                Some(old_path),
                new_blob.as_ref(),
                Some(new_path),
                None,
                None,
                None,
                None,
                Some(&mut line_callback),
            )
            .map_err(git_error(format!("cannot diff Git blobs for `{new_path}`")))?;
        Ok(coalesce_ranges(ranges))
    }

    fn tree_blob(&self, reference: &str, path: &str) -> Result<Option<git2::Blob<'_>>> {
        let entry = match self.tree_at(reference)?.get_path(Path::new(path)) {
            Ok(entry) => entry,
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(error) => {
                return Err(CrivError::new(format!(
                    "git ref `{reference}` cannot read `{path}`: {error}"
                )));
            }
        };
        self.repository
            .find_blob(entry.id())
            .map(Some)
            .map_err(git_error(format!(
                "git ref `{reference}` does not contain blob `{path}`"
            )))
    }

    /// Every path that differs between `HEAD`, the index, and the worktree,
    /// including untracked files and both sides of a rename.
    pub(crate) fn dirty_paths(&self) -> Result<Vec<String>> {
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);
        let statuses = self
            .repository
            .statuses(Some(&mut options))
            .map_err(git_error("Git status failed"))?;
        let mut paths = Vec::new();
        for status in statuses.iter() {
            if let Ok(path) = status.path() {
                paths.push(path.to_string());
            }
            for delta in [status.head_to_index(), status.index_to_workdir()]
                .into_iter()
                .flatten()
            {
                for file in [delta.old_file(), delta.new_file()] {
                    if let Some(path) = file.path() {
                        paths.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    /// Fails when the repository has no `user.name` and `user.email`.
    pub(crate) fn preflight_commit_identity(&self) -> Result<()> {
        self.signature().map(|_| ())
    }

    /// Stages each path as the worktree holds it, or its deletion when the
    /// worktree has no file there.
    pub(crate) fn stage_paths(&self, paths: &[String]) -> Result<()> {
        let mut index = self.index()?;
        for path in paths {
            match index.add_path(Path::new(path)) {
                Ok(()) => {}
                Err(error) if error.code() == git2::ErrorCode::NotFound => index
                    .remove_path(Path::new(path))
                    .map_err(git_error(format!(
                        "Git could not stage deletion of `{path}`"
                    )))?,
                Err(error) => {
                    return Err(CrivError::new(format!(
                        "Git could not stage `{path}`: {error}"
                    )));
                }
            }
        }
        index
            .write()
            .map_err(git_error("Git index could not be written"))
    }

    /// Commits the index on `HEAD` with the configured identity.
    pub(crate) fn commit_staged(&self, message: &str) -> Result<String> {
        let signature = self.signature()?;
        let parent = self
            .repository
            .head()
            .and_then(|head| head.peel_to_commit())
            .map_err(git_error("Git HEAD could not be read"))?;
        let tree_id = self
            .index()?
            .write_tree()
            .map_err(git_error("Git index tree could not be written"))?;
        let tree = self
            .repository
            .find_tree(tree_id)
            .map_err(git_error("Git index tree could not be read"))?;
        self.repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[&parent],
            )
            .map(|oid| oid.to_string())
            .map_err(git_error("Git commit failed"))
    }

    fn signature(&self) -> Result<git2::Signature<'static>> {
        self.repository.signature().map_err(git_error(
            "Git commit identity is unavailable; configure `user.name` and `user.email`",
        ))
    }

    fn index(&self) -> Result<git2::Index> {
        self.repository
            .index()
            .map_err(git_error("Git index could not be read"))
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
            .map_err(git_error(format!(
                "git ref `{reference}` does not resolve to a tree"
            )))
    }

    fn commit_at(&self, reference: &str) -> Result<git2::Commit<'_>> {
        self.repository
            .revparse_single(reference)
            .and_then(|object| object.peel_to_commit())
            .map_err(git_error(format!(
                "git ref `{reference}` does not resolve to a commit"
            )))
    }
}

fn changed_entries(
    mut diff: git2::Diff<'_>,
    old_ref: Option<&str>,
    new: &Revision,
) -> Result<Vec<ChangedEntry>> {
    let mut similarity = git2::DiffFindOptions::new();
    similarity
        .renames(true)
        .copies(true)
        .copies_from_unmodified(true)
        .remove_unmodified(true)
        .rename_threshold(50)
        .copy_threshold(100);
    diff.find_similar(Some(&mut similarity))
        .map_err(git_error("git similarity detection failed"))?;

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
            new: new.clone(),
        });
    }
    Ok(entries)
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

pub fn is_zero_oid(oid: &str) -> bool {
    oid.bytes().all(|byte| byte == b'0')
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

/// Git CLI fixture builders shared by unit tests.
#[cfg(test)]
pub(crate) mod fixture {
    pub(crate) use super::tests::{git, git_with_input};
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// Runs the Git CLI in `root` as a fixture builder, isolated from any
    /// repository the test process itself runs in, and returns its trimmed
    /// standard output.
    pub(crate) fn git(root: &Path, args: &[&str]) -> String {
        git_with_input(root, args, "")
    }

    pub(crate) fn git_with_input(root: &Path, args: &[&str], input: &str) -> String {
        let mut command = Command::new("git");
        command.current_dir(root);
        for variable in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_PREFIX",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ] {
            command.env_remove(variable);
        }
        let mut child = command
            .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .args(["-c", "user.name=criv", "-c", "user.email=criv@example.com"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("git command should run");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn revisions_report_absent_files_and_staging_records_deletions() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        git(root, &["init"]);
        std::fs::write(root.join("kept.txt"), "kept\n").unwrap();
        std::fs::write(root.join("gone.txt"), "gone\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "base"]);
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        let repository = super::GitRepository::open(root).unwrap();
        let head = super::Revision::Commit("HEAD".into());

        for revision in [&head, &super::Revision::Index, &super::Revision::Worktree] {
            assert!(repository.file(revision, "missing.txt").unwrap().is_none());
            assert_eq!(
                repository.file(revision, "kept.txt").unwrap().unwrap().mode,
                "100644"
            );
        }
        assert!(
            repository
                .file(&super::Revision::Worktree, "gone.txt")
                .unwrap()
                .is_none()
        );
        repository
            .stage_paths(&["gone.txt".into(), "kept.txt".into()])
            .unwrap();
        assert_eq!(
            git(root, &["diff", "--cached", "--name-status"]),
            "D\tgone.txt"
        );
    }
}

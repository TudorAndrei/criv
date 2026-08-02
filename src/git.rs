//! Small, fail-closed wrappers around the Git CLI.
//!
//! Keeping these calls in one module prevents hook environment variables from
//! changing the repository a command inspects.

use std::ops::Range;
use std::path::Path;
use std::process::{Command, Output};

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

/// Builds a Git command rooted in the requested vault, independent of any
/// repository context inherited from a Git hook.
fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_PREFIX");
    command
}

pub(crate) fn output(root: &Path, args: &[&str]) -> Result<Output> {
    let output = command(root)
        .args(args)
        .output()
        .map_err(|err| CrivError::new(format!("failed to run `git {}`: {err}", args.join(" "))))?;
    if output.status.success() {
        return Ok(output);
    }
    Err(CrivError::new(format!(
        "`git {}` failed with {}: {}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

pub(crate) fn is_repository(root: &Path) -> Result<bool> {
    let output = command(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|err| {
            CrivError::new(format!(
                "failed to run `git rev-parse --is-inside-work-tree`: {err}"
            ))
        })?;
    Ok(output.status.success() && output.stdout == b"true\n")
}

pub(crate) fn changed_set(
    root: &Path,
    args: &[&str],
    old_ref: Option<&str>,
    new_ref: Option<&str>,
) -> Result<ChangedSet> {
    let output = output(root, args)?;
    let mut entries = parse_changed_entries(&output.stdout)?;
    for entry in &mut entries {
        entry.old_ref = old_ref.map(str::to_string);
        entry.new_ref = new_ref.map(str::to_string);
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

/// Compares two committed revisions with explicit rename and copy detection.
/// This deliberately ignores user-level `diff.renames` settings because
/// ownership proofs must be reproducible in hooks and CI.
pub(crate) fn changes_between(root: &Path, old: &str, new: &str) -> Result<ChangedSet> {
    changes_between_paths(root, old, new, &[])
}

/// Like [`changes_between`], but restricts Git's traversal to relevant paths.
/// Callers proving ADR identity use this to avoid treating unrelated repository
/// changes as allocation evidence.
pub(crate) fn changes_between_paths(
    root: &Path,
    old: &str,
    new: &str,
    paths: &[&str],
) -> Result<ChangedSet> {
    let mut args = vec![
        "diff",
        "--name-status",
        "-z",
        "--find-renames=50%",
        "--find-copies=100%",
        "--find-copies-harder",
        old,
        new,
    ];
    if !paths.is_empty() {
        args.push("--");
        args.extend_from_slice(paths);
    }
    changed_set(root, &args, Some(old), Some(new))
}

/// Compares HEAD with the index and working tree using the same explicit move
/// detection as committed comparisons. This is evidence of the current input,
/// not a substitute for the merge-base ownership proof.
pub(crate) fn worktree_changes_in(root: &Path, paths: &[&str]) -> Result<ChangedSet> {
    let mut args = vec![
        "diff",
        "--name-status",
        "-z",
        "--find-renames=50%",
        "--find-copies=100%",
        "--find-copies-harder",
        "HEAD",
    ];
    if !paths.is_empty() {
        args.push("--");
        args.extend_from_slice(paths);
    }
    changed_set(root, &args, Some("HEAD"), None)
}

pub(crate) fn parse_changed_entries(stdout: &[u8]) -> Result<Vec<ChangedEntry>> {
    let mut entries = Vec::new();
    let mut fields = stdout
        .split(|byte| *byte == b'\0')
        .filter(|field| !field.is_empty());
    while let Some(status_field) = fields.next() {
        let status = status_field.first().copied().ok_or_else(|| {
            CrivError::new("Git name-status output contained an empty status field")
        })? as char;
        let status = match status {
            'A' => ChangeStatus::Added,
            'M' | 'T' => ChangeStatus::Modified,
            'D' => ChangeStatus::Deleted,
            'R' => ChangeStatus::Renamed,
            'C' => ChangeStatus::Copied,
            _ => ChangeStatus::Other,
        };
        let (path, previous_path) = match status {
            ChangeStatus::Renamed | ChangeStatus::Copied => {
                let previous_path = next_path(&mut fields)?;
                let path = next_path(&mut fields)?;
                (path, Some(previous_path))
            }
            _ => (next_path(&mut fields)?, None),
        };
        entries.push(ChangedEntry {
            status,
            path,
            previous_path,
            old_ref: None,
            new_ref: None,
        });
    }
    Ok(entries)
}

fn next_path<'a>(fields: &mut impl Iterator<Item = &'a [u8]>) -> Result<String> {
    let value = fields
        .next()
        .ok_or_else(|| CrivError::new("Git name-status output ended before a path field"))?;
    String::from_utf8(value.to_vec()).map_err(|_| {
        CrivError::new("Git changed path is not valid UTF-8; criv cannot represent it")
    })
}

/// Resolve an arbitrary ref once and return the complete commit object ID.
pub(crate) fn resolve_commit(root: &Path, git_ref: &str) -> Result<String> {
    let object = format!("{git_ref}^{{commit}}");
    let output = output(root, &["rev-parse", "--verify", &object]).map_err(|_| {
        CrivError::new(format!(
            "cannot resolve base ref `{git_ref}` to a commit; fetch complete history and retry"
        ))
    })?;
    let sha = String::from_utf8(output.stdout)
        .map_err(|_| CrivError::new("Git commit object ID is not valid UTF-8"))?;
    let sha = sha.trim();
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CrivError::new(format!(
            "Git resolved `{git_ref}` to an invalid commit ID"
        )));
    }
    Ok(sha.to_owned())
}

pub(crate) fn merge_base(root: &Path, first: &str, second: &str) -> Result<String> {
    let output = output(root, &["merge-base", first, second]).map_err(|_| {
        CrivError::new(format!("cannot find a merge base for `{first}` and `{second}`; fetch complete history and retry"))
    })?;
    let sha = String::from_utf8(output.stdout)
        .map_err(|_| CrivError::new("Git merge-base output is not valid UTF-8"))?;
    let sha = sha.trim();
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CrivError::new(
            "Git merge-base returned an invalid commit ID",
        ));
    }
    Ok(sha.to_owned())
}

pub(crate) fn tree_paths(root: &Path, commit: &str, prefix: &str) -> Result<Vec<String>> {
    let output = output(
        root,
        &["ls-tree", "-r", "-z", "--name-only", commit, "--", prefix],
    )?;
    output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|field| !field.is_empty())
        .map(|field| {
            String::from_utf8(field.to_vec()).map_err(|_| {
                CrivError::new("Git tree path is not valid UTF-8; criv cannot represent it")
            })
        })
        .collect()
}

pub(crate) fn blob(root: &Path, git_ref: &str, path: &str) -> Result<String> {
    let object = if git_ref == ":" {
        format!(":{path}")
    } else {
        format!("{git_ref}:{path}")
    };
    let output = output(root, &["show", &object])?;
    String::from_utf8(output.stdout).map_err(|_| {
        CrivError::new(format!(
            "Git blob `{object}` is not valid UTF-8; reconciliation only supports text files"
        ))
    })
}

pub(crate) fn file_mode(root: &Path, git_ref: &str, path: &str) -> Result<Option<String>> {
    let output = if git_ref == ":" {
        output(root, &["ls-files", "-s", "-z", "--", path])?
    } else {
        output(root, &["ls-tree", "-z", git_ref, "--", path])?
    };
    let Some(record) = output.stdout.split(|byte| *byte == b'\0').next() else {
        return Ok(None);
    };
    if record.is_empty() {
        return Ok(None);
    }
    let mode = record
        .split(|byte| *byte == b' ')
        .next()
        .ok_or_else(|| CrivError::new("Git mode output was malformed"))?;
    let mode = String::from_utf8(mode.to_vec())
        .map_err(|_| CrivError::new("Git mode output is not valid UTF-8"))?;
    (mode.len() == 6 && mode.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(mode)
        .map(Some)
        .ok_or_else(|| CrivError::new("Git mode output was malformed"))
}

/// Zero-context line ownership for content newly added between two revisions.
pub(crate) fn added_lines(
    root: &Path,
    old: &str,
    new: &str,
    path: &str,
) -> Result<Vec<Range<usize>>> {
    let output = output(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--unified=0",
            "--no-color",
            old,
            new,
            "--",
            path,
        ],
    )?;
    parse_added_lines(&output.stdout, path)
}

/// Zero-context line ownership between two committed blobs whose paths differ.
/// Comparing blob specifications directly also handles a modified copy whose
/// source path remains present in the new tree and a low-similarity move that
/// Git reports as deletion/addition.
pub(crate) fn added_lines_between_blobs(
    root: &Path,
    old: &str,
    old_path: &str,
    new: &str,
    new_path: &str,
) -> Result<Vec<Range<usize>>> {
    let old_blob = format!("{old}:{old_path}");
    let new_blob = format!("{new}:{new_path}");
    let output = output(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--unified=0",
            "--no-color",
            &old_blob,
            &new_blob,
        ],
    )?;
    parse_added_lines(&output.stdout, new_path)
}

fn parse_added_lines(stdout: &[u8], path: &str) -> Result<Vec<Range<usize>>> {
    let text = String::from_utf8(stdout.to_vec())
        .map_err(|_| CrivError::new("Git diff output is not valid UTF-8"))?;
    let mut ranges = Vec::new();
    for line in text.lines() {
        if !line.starts_with("@@ ") {
            continue;
        }
        let plus = line
            .split_whitespace()
            .find(|part| part.starts_with('+'))
            .ok_or_else(|| {
                CrivError::new(format!(
                    "cannot parse Git diff hunk while proving ownership of `{path}`"
                ))
            })?;
        let span = &plus[1..];
        let (start, length) = match span.split_once(',') {
            Some((start, length)) => (start, length),
            None => (span, "1"),
        };
        let start = start.parse::<usize>().map_err(|_| {
            CrivError::new(format!(
                "cannot parse Git diff hunk while proving ownership of `{path}`"
            ))
        })?;
        let length = length.parse::<usize>().map_err(|_| {
            CrivError::new(format!(
                "cannot parse Git diff hunk while proving ownership of `{path}`"
            ))
        })?;
        if length > 0 {
            ranges.push(start..start + length);
        }
    }
    Ok(ranges)
}

pub(crate) fn dirty_paths(root: &Path) -> Result<Vec<String>> {
    let output = output(root, &["status", "--porcelain=v1", "-z"])?;
    let mut fields = output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|field| !field.is_empty());
    let mut paths = Vec::new();
    while let Some(field) = fields.next() {
        if field.len() < 4 || field[2] != b' ' {
            return Err(CrivError::new("Git status output was malformed"));
        }
        let renamed_or_copied = matches!(field[0], b'R' | b'C') || matches!(field[1], b'R' | b'C');
        let path = String::from_utf8(field[3..].to_vec()).map_err(|_| {
            CrivError::new("Git status path is not valid UTF-8; criv cannot represent it")
        })?;
        paths.push(path);
        if renamed_or_copied {
            let previous = fields
                .next()
                .ok_or_else(|| CrivError::new("Git status rename output was malformed"))?;
            paths.push(String::from_utf8(previous.to_vec()).map_err(|_| {
                CrivError::new("Git status path is not valid UTF-8; criv cannot represent it")
            })?);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Restore only the named paths in the index to HEAD. Callers use this for a
/// receipt they have already proved complete; it never touches worktree data.
pub(crate) fn reset_index_paths(root: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["reset", "--quiet", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    output(root, &args).map(|_| ())
}

pub(crate) fn ref_is_stable(root: &Path, git_ref: &str, expected_sha: &str) -> Result<bool> {
    Ok(resolve_commit(root, git_ref)? == expected_sha)
}

pub(crate) fn first_parent(root: &Path, commit: &str) -> Result<Option<String>> {
    let output = command(root)
        .args(["rev-parse", "--verify", &format!("{commit}^")])
        .output()
        .map_err(|err| {
            CrivError::new(format!("failed to run git rev-parse for {commit}: {err}"))
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    String::from_utf8(output.stdout)
        .map_err(|_| CrivError::new("Git parent object ID is not valid UTF-8"))
        .map(|stdout| Some(stdout.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_delimited_rename_and_copy_entries() {
        let entries =
            parse_changed_entries(b"R100\0old.md\0new.md\0C100\0one.md\0two.md\0").unwrap();
        assert_eq!(entries[0].status, ChangeStatus::Renamed);
        assert_eq!(entries[0].previous_path.as_deref(), Some("old.md"));
        assert_eq!(entries[1].status, ChangeStatus::Copied);
    }

    #[test]
    fn rejects_non_utf8_name_status_path() {
        assert!(parse_changed_entries(b"M\0bad-\xff\0").is_err());
    }
}

mod reconcile_transaction;
mod source_reconcile;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use usage::{Args as UsageArgs, Subcommands};

use crate::config::Config;
use crate::git::{self, ChangeStatus, ChangedEntry, ChangedSet};
use crate::repository::RepositoryFiles;
use crate::vault::Vault;
use crate::{CrivError, Result};

use self::reconcile_transaction::Snapshot;

use source_reconcile::{
    allows_history_change as source_history_change_is_allowed,
    receipt_allows_transaction as source_receipt_allows_transaction,
    receipt_is_current as source_receipt_is_current,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChangeMode {
    Commit,
    Push,
    Ci,
}

/// Validate the complete selected ADR transaction in enforcement order.
pub fn change_violations(
    files: &RepositoryFiles,
    config: &Config,
    changes: Option<&git::ChangedSet>,
    mode: ChangeMode,
) -> Vec<String> {
    let root = files.root();
    let receipt_is_current = receipt_is_current(root);
    let receipt_allows_transaction = mode == ChangeMode::Commit
        && changes.is_some_and(|changes| receipt_allows_transaction(root, &changes.entries));
    let source_receipt_is_current = source_receipt_is_current(root);
    let source_receipt_allows_transaction = mode == ChangeMode::Commit
        && changes.is_some_and(|changes| source_receipt_allows_transaction(root, &changes.entries));
    let mut adr_violations = adr_immutability_violations(
        &config.docs_dir,
        &config.adr_dir,
        changes.map(|changes| changes.entries.as_slice()),
        |entry| {
            (receipt_allows_transaction
                || source_receipt_allows_transaction
                || is_allowed_adr_change(files, changes, entry))
                || (mode == ChangeMode::Ci && is_branch_local_ci_change(root, entry))
        },
    );
    if receipt_is_current && !receipt_allows_transaction {
        adr_violations.push(
            "ADR reconciliation receipt does not prove the complete staged transaction".into(),
        );
    }
    if source_receipt_is_current && !source_receipt_allows_transaction {
        adr_violations.push(
            "source reconciliation receipt does not prove the complete staged transaction".into(),
        );
    }
    adr_violations.extend(config_scope_violations(root, changes, config));
    adr_violations
}

/// Report protected ADR edits in Git entry order, after testing allowed exceptions.
fn adr_immutability_violations(
    docs_dir: &str,
    adr_dir: &str,
    changed_entries: Option<&[ChangedEntry]>,
    mut is_allowed_change: impl FnMut(&ChangedEntry) -> bool,
) -> Vec<String> {
    let Some(entries) = changed_entries else {
        return Vec::new();
    };

    let mut violations = Vec::new();
    for entry in entries {
        if matches!(entry.status, ChangeStatus::Added | ChangeStatus::Copied) {
            continue;
        }

        let path = entry.previous_path.as_deref().unwrap_or(&entry.path);
        if !is_adr_file(docs_dir, adr_dir, path) {
            continue;
        }
        if is_allowed_change(entry) {
            continue;
        }

        let display_path = entry.previous_path.as_ref().map_or_else(
            || entry.path.clone(),
            |previous| format!("{previous} -> {}", entry.path),
        );
        violations.push(format!(
            "{display_path}: ADR files are immutable; add a new ADR with `supersedes` instead of modifying an existing one"
        ));
    }
    violations
}

/// Accept receipt-proven changes or an exact portable-link migration.
fn is_allowed_adr_change(
    files: &RepositoryFiles,
    changes: Option<&ChangedSet>,
    entry: &ChangedEntry,
) -> bool {
    let root = files.root();
    if changes.is_some_and(|changes| source_history_change_is_allowed(root, changes, entry)) {
        return true;
    }
    // A committed receipt proves the entire tree transition, including paths
    // that Git reports as modifications when ADR mappings overlap.
    if entry
        .new_ref
        .as_deref()
        .is_some_and(|commit| receipt_allows_commit(root, commit))
    {
        return true;
    }
    if changes.is_some_and(|changes| receipt_allows_history_change(root, changes, entry)) {
        return true;
    }
    if matches!(entry.status, ChangeStatus::Renamed | ChangeStatus::Deleted)
        && receipt_allows_change(root, entry)
    {
        return true;
    }
    if entry.status != ChangeStatus::Modified {
        return false;
    }
    let Some(old) = read_changed_content(files, entry.old_ref.as_deref(), &entry.path) else {
        return false;
    };
    let Some(new) = read_changed_content(files, entry.new_ref.as_deref(), &entry.path) else {
        return false;
    };
    is_mechanical_wikilink_portability_migration(&old, &new)
}

/// A CI comparison can contain a deletion for a target path that did not exist
/// at the branch/target merge base: that is the same-path allocation race, not
/// an edit of a published ADR. The merge-base proof is deliberately narrow;
/// anything present at that base stays immutable.
fn is_branch_local_ci_change(root: &Path, entry: &ChangedEntry) -> bool {
    let Some(target) = entry.old_ref.as_deref() else {
        return false;
    };
    let path = entry.previous_path.as_deref().unwrap_or(&entry.path);
    let Ok(merge_base) = git::merge_base(root, target, "HEAD") else {
        return false;
    };
    git::tree_paths(root, &merge_base, path)
        .is_ok_and(|paths| !paths.iter().any(|candidate| candidate == path))
}

/// Read proof from Git or from a confined, regular working-tree file.
fn read_changed_content(
    files: &RepositoryFiles,
    git_ref: Option<&str>,
    path: &str,
) -> Option<String> {
    let Some(git_ref) = git_ref else {
        return files.read_string(Path::new(path)).ok();
    };
    git::blob(files.root(), git_ref, path).ok()
}

/// Require the new text to differ only by equivalent portable ADR links.
fn is_mechanical_wikilink_portability_migration(old: &str, new: &str) -> bool {
    old != new && normalize_portable_adr_links(new) == old
}

/// Restore short ADR aliases while preserving all text outside matching links.
fn normalize_portable_adr_links(markdown: &str) -> String {
    let mut normalized = String::with_capacity(markdown.len());
    let mut start = 0;
    while let Some(tail) = markdown.get(start..) {
        let Some(relative_open) = tail.find("[[") else {
            break;
        };
        let Some(open) = start.checked_add(relative_open) else {
            break;
        };
        let Some(body_start) = open.checked_add(2) else {
            break;
        };
        let Some(body_tail) = markdown.get(body_start..) else {
            break;
        };
        let Some(relative_close) = body_tail.find("]]") else {
            break;
        };
        let Some(close) = body_start.checked_add(relative_close) else {
            break;
        };
        let Some(end) = close.checked_add(2) else {
            break;
        };
        let (Some(prefix), Some(body), Some(link)) = (
            markdown.get(start..open),
            markdown.get(body_start..close),
            markdown.get(open..end),
        ) else {
            break;
        };
        normalized.push_str(prefix);
        if let Some(alias) = portable_adr_link_alias(body) {
            normalized.push_str("[[");
            normalized.push_str(alias);
            normalized.push_str("]]");
        } else {
            normalized.push_str(link);
        }
        start = end;
    }
    normalized.push_str(markdown.get(start..).unwrap_or_default());
    normalized
}

/// Return the ADR alias only when its ID and fragment match the target.
fn portable_adr_link_alias(body: &str) -> Option<&str> {
    let (target, alias) = body.split_once('|')?;
    let alias = alias.trim();
    let alias_base = alias.split('#').next().unwrap_or(alias);
    if !crate::identity::is_adr_id(alias_base) {
        return None;
    }
    let target_fragment = target.split_once('#').map(|(_, fragment)| fragment);
    let alias_fragment = alias.split_once('#').map(|(_, fragment)| fragment);
    if target_fragment != alias_fragment {
        return None;
    }
    let number = alias_base.get(4..)?;
    let target_base = target.split('#').next().unwrap_or(target).trim();
    let basename = target_base
        .trim_end_matches(".md")
        .split('/')
        .next_back()
        .unwrap_or(target_base);
    (basename == number || basename.starts_with(&format!("{number}-"))).then_some(alias)
}

/// Reject a decision edit that also moves the configured decision directory.
fn config_scope_violations(
    root: &Path,
    changes: Option<&ChangedSet>,
    current: &Config,
) -> Vec<String> {
    let Some(changes) = changes else {
        return Vec::new();
    };
    let paths = || {
        changes
            .entries
            .iter()
            .flat_map(|entry| [Some(entry.path.as_str()), entry.previous_path.as_deref()])
            .flatten()
    };
    if !paths().any(|path| path == "criv.toml") {
        return Vec::new();
    }
    if !paths().any(looks_like_decision) {
        return Vec::new();
    }
    let Some(old_ref) = changes.old_ref.as_deref() else {
        return Vec::new();
    };
    let Ok(previous) =
        git::blob(root, old_ref, "criv.toml").and_then(|contents| Config::parse(Some(&contents)))
    else {
        return Vec::new();
    };
    if previous.docs_dir == current.docs_dir && previous.adr_dir == current.adr_dir {
        return Vec::new();
    }
    vec![format!(
        "criv.toml moves the decision scope from `{}/{}` to `{}/{}` in the same transaction as a decision change; the immutability gate would read the new scope",
        previous.docs_dir, previous.adr_dir, current.docs_dir, current.adr_dir
    )]
}

/// Recognize decision-shaped Markdown paths independently of the current scope.
fn looks_like_decision(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "md")
        && path.split('/').any(|component| component == "adr")
        && !path.ends_with("/README.md")
}

/// Match Markdown decisions under the configured scope, excluding its index.
fn is_adr_file(docs_dir: &str, adr_dir: &str, path: &str) -> bool {
    let adr_prefix = format!("{docs_dir}/{adr_dir}/");
    path.starts_with(&adr_prefix)
        && path != format!("{adr_prefix}README.md")
        && Path::new(path)
            .extension()
            .is_some_and(|extension| extension == "md")
}

const RECEIPT_SCHEMA: &str = "criv.adr-reconcile/3";
const RECEIPT_PATH: &str = ".criv/adr-reconcile.json";
const RECONCILIATION_COMMIT_MESSAGE: &str = "docs(adr): reconcile provisional identifiers";

#[derive(Debug, UsageArgs)]
pub struct AdrOptions {
    #[usage(subcommand)]
    command: AdrCommand,
}

#[derive(Debug, Subcommands)]
enum AdrCommand {
    /// Reconcile provisional ADR IDs against an integration target.
    Reconcile(ReconcileOptions),
    /// Reconcile exact governed source renames against an integration target.
    ReconcileSources(source_reconcile::Options),
}

#[derive(Debug, UsageArgs)]
struct ReconcileOptions {
    /// Target branch or commit to compare with.
    #[usage(long)]
    base: String,
    /// Report a required reconciliation without modifying the worktree.
    #[usage(long)]
    check: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct AdrFile {
    path: String,
    id: u32,
    slug: String,
    contents: String,
}

#[derive(Debug, Clone)]
struct ReconcilePlan {
    base_ref: String,
    head_sha: String,
    target_sha: String,
    merge_base: String,
    /// Paths and IDs currently materialized in the worktree. These are the
    /// inputs to rewriting, which can differ from the committed branch
    /// identity after an earlier receipt has been applied.
    mappings: Vec<Mapping>,
    /// The equivalent transaction expressed against `head_sha`. Receipts
    /// always describe the eventual index/commit relative to that commit.
    receipt_mappings: Vec<Mapping>,
    branch_adrs: Vec<AdrFile>,
    receipt_sources: Vec<AdrFile>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct Mapping {
    old_id: u32,
    new_id: u32,
    old_path: String,
    new_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Receipt {
    schema: String,
    base_ref: String,
    head_sha: String,
    target_sha: String,
    merge_base: String,
    mappings: Vec<Mapping>,
    sources: Vec<ReceiptSource>,
    deletions: Vec<String>,
    files: Vec<ReceiptFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReceiptFile {
    path: String,
    before_hash: Option<String>,
    after_hash: String,
    before_mode: Option<String>,
    after_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReceiptSource {
    path: String,
    before_hash: String,
    before_mode: String,
}

pub fn run(root: &Path, options: &AdrOptions) -> Result<()> {
    RepositoryFiles::open_vault(root)?;
    match &options.command {
        AdrCommand::Reconcile(options) => reconcile(root, options),
        AdrCommand::ReconcileSources(options) => source_reconcile::run(root, options),
    }
}

/// CI calls the same read-only planner as the user-facing `--check` command.
pub fn check_base(root: &Path, base_ref: &str) -> Result<()> {
    reconcile(
        root,
        &ReconcileOptions {
            base: base_ref.to_owned(),
            check: true,
        },
    )
}

/// Local hooks accept only the complete staged transaction produced by this
/// command. Git may present its ADR move as either a rename or a deletion.
fn receipt_allows_change(root: &Path, entry: &git::ChangedEntry) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    if git::resolve_commit(root, "HEAD").ok().as_deref() != Some(receipt.head_sha.as_str())
        || !receipt_common_matches(root, &receipt)
        || !receipt_tree_matches(root, &receipt, ":")
    {
        return false;
    }
    match entry.status {
        git::ChangeStatus::Renamed => entry.previous_path.as_deref().is_some_and(|old_path| {
            receipt
                .mappings
                .iter()
                .any(|mapping| mapping.old_path == old_path && mapping.new_path == entry.path)
        }),
        git::ChangeStatus::Deleted => receipt.deletions.iter().any(|path| path == &entry.path),
        _ => false,
    }
}

/// A receipt is relevant only to the exact commit from which reconciliation
/// started. A later commit leaves the ignored receipt behind harmlessly.
fn receipt_is_current(root: &Path) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    receipt.schema == RECEIPT_SCHEMA
        && git::resolve_commit(root, "HEAD").ok().as_deref() == Some(receipt.head_sha.as_str())
}

/// Reject partial staging even when Git has no deletion entry left to send
/// through the per-ADR immutability gate (for example, after a source ADR is
/// recreated at its former path).
fn receipt_allows_transaction(root: &Path, entries: &[git::ChangedEntry]) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    git::resolve_commit(root, "HEAD").ok().as_deref() == Some(receipt.head_sha.as_str())
        && receipt_common_matches(root, &receipt)
        && receipt_tree_matches(root, &receipt, ":")
        && receipt_paths_match(&receipt, entries)
}

/// A reconciliation receipt may prove exactly one committed transaction after
/// its planning HEAD. This is used for push enforcement, where the receipt is
/// intentionally ignored and the checkout may have advanced past that commit.
fn receipt_allows_commit(root: &Path, commit: &str) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    let Ok(Some(parent)) = git::first_parent(root, commit) else {
        return false;
    };
    if parent != receipt.head_sha || !receipt_common_matches(root, &receipt) {
        return false;
    }
    let Ok(entries) = git::changes_between(root, &parent, commit) else {
        return false;
    };
    receipt_tree_matches(root, &receipt, commit) && receipt_paths_match(&receipt, &entries.entries)
}

/// A combined push comparison may end at a later commit or merge. Admit only
/// receipt paths whose exact transaction is present in the compared history
/// and whose generated outputs have not changed since that commit.
fn receipt_allows_history_change(
    root: &Path,
    changes: &git::ChangedSet,
    entry: &git::ChangedEntry,
) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    let (Some(old_ref), Some(new_ref)) = (changes.old_ref.as_deref(), changes.new_ref.as_deref())
    else {
        return false;
    };
    let Ok(commits) = git::commits_between(root, old_ref, new_ref) else {
        return false;
    };
    if !commits
        .iter()
        .any(|commit| receipt_allows_commit(root, commit))
        || !receipt_outputs_survive(root, &receipt, new_ref)
    {
        return false;
    }

    let expected = receipt
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(receipt.deletions.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    expected.contains(entry.path.as_str())
        && entry
            .previous_path
            .as_deref()
            .is_none_or(|path| expected.contains(path))
}

fn receipt_common_matches(root: &Path, receipt: &Receipt) -> bool {
    receipt.schema == RECEIPT_SCHEMA
        && git::ref_is_stable(root, &receipt.base_ref, &receipt.target_sha).unwrap_or(false)
        && receipt.sources.iter().all(|source| {
            git::blob(root, &receipt.head_sha, &source.path)
                .is_ok_and(|contents| hash(&contents) == source.before_hash)
                && git::file_mode(root, &receipt.head_sha, &source.path)
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(source.before_mode.as_str())
        })
}

fn receipt_tree_matches(root: &Path, receipt: &Receipt, tree: &str) -> bool {
    receipt.files.iter().all(|file| {
        let before_matches = file.before_hash.as_ref().map_or_else(
            || git::blob(root, &receipt.head_sha, &file.path).is_err(),
            |before_hash| {
                git::blob(root, &receipt.head_sha, &file.path)
                    .is_ok_and(|contents| hash(&contents) == *before_hash)
            },
        );
        before_matches
            && git::file_mode(root, &receipt.head_sha, &file.path)
                .ok()
                .flatten()
                .as_deref()
                == file.before_mode.as_deref()
            && git::blob(root, tree, &file.path)
                .is_ok_and(|contents| hash(&contents) == file.after_hash)
            && git::file_mode(root, tree, &file.path)
                .ok()
                .flatten()
                .as_deref()
                == Some(file.after_mode.as_str())
    }) && receipt.deletions.iter().all(|path| {
        !receipt.files.iter().any(|file| file.path == *path) && git::blob(root, tree, path).is_err()
    })
}

fn receipt_outputs_survive(root: &Path, receipt: &Receipt, tree: &str) -> bool {
    receipt.files.iter().all(|file| {
        git::blob(root, tree, &file.path).is_ok_and(|contents| hash(&contents) == file.after_hash)
            && git::file_mode(root, tree, &file.path)
                .ok()
                .flatten()
                .as_deref()
                == Some(file.after_mode.as_str())
    }) && receipt.deletions.iter().all(|path| {
        let final_blob = git::blob(root, tree, path);
        let target_blob = git::blob(root, &receipt.target_sha, path);
        match (final_blob, target_blob) {
            (Err(_), _) => true,
            (Ok(final_blob), Ok(target_blob)) => {
                final_blob == target_blob
                    && git::file_mode(root, tree, path).ok().flatten()
                        == git::file_mode(root, &receipt.target_sha, path)
                            .ok()
                            .flatten()
            }
            (Ok(_), Err(_)) => false,
        }
    })
}

fn receipt_paths_match(receipt: &Receipt, entries: &[git::ChangedEntry]) -> bool {
    let expected = receipt
        .files
        .iter()
        .map(|file| file.path.clone())
        .chain(receipt.deletions.iter().cloned())
        .collect::<BTreeSet<_>>();
    let actual = entries
        .iter()
        .flat_map(|entry| std::iter::once(entry.path.clone()).chain(entry.previous_path.clone()))
        .collect::<BTreeSet<_>>();
    actual == expected
}

fn reconcile(root: &Path, options: &ReconcileOptions) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    if !git::is_repository(root)? {
        return Err(CrivError::new(
            "`criv adr reconcile` requires a Git worktree",
        ));
    }
    let target_sha = git::resolve_commit(root, &options.base)?;
    let materialized = current_materialized_receipt(root)?;
    let plan = build_plan_from(&files, &options.base, &target_sha)?;
    println!("ADR reconciliation target: {}", plan.target_sha);
    if plan.mappings.is_empty() {
        println!("ADR allocation is current; no reconciliation is required");
        return Ok(());
    }
    print_mapping(&plan.mappings);
    if options.check {
        return Err(CrivError::new(format!(
            "ADR allocation conflicts with target {}; run `criv adr reconcile --base {}`",
            plan.target_sha, options.base
        )));
    }
    let dirty = git::dirty_paths(root)?;
    if !dirty.is_empty() && materialized.is_none() {
        return Err(CrivError::new(format!(
            "refusing to reconcile a dirty worktree; commit or stash: {}",
            dirty.join(", ")
        )));
    }
    git::preflight_commit_identity(root)?;
    if !git::ref_is_stable(root, &options.base, &plan.target_sha)? {
        return Err(CrivError::new(format!(
            "target ref `{}` moved since it resolved to {}; retry reconciliation",
            options.base, plan.target_sha
        )));
    }
    let transaction_paths = transaction_paths(root, &plan)?;
    let rollback_paths = transaction_paths
        .iter()
        .cloned()
        .chain(std::iter::once(RECEIPT_PATH.to_string()))
        .collect::<Vec<_>>();
    let snapshot = Snapshot::capture_from(&files, &rollback_paths)?;
    let commit = (|| {
        apply_plan(&files, &plan)?;
        if !git::ref_is_stable(root, &options.base, &plan.target_sha)? {
            return Err(CrivError::new(format!(
                "target ref `{}` moved during reconciliation; retry against its new SHA",
                options.base
            )));
        }
        // Clear any staged remnants from a previously materialized receipt as
        // well as staging the newly planned transaction before proving it.
        git::stage_paths(root, &transaction_paths)?;
        let receipt = materialized_receipt(root)?;
        let paths = receipt_paths(&receipt);
        git::stage_paths(root, &paths)?;
        let staged = git::staged_changes(root)?;
        if !receipt_allows_transaction(root, &staged.entries) {
            return Err(CrivError::new(
                "ADR reconciliation receipt does not prove the complete staged transaction",
            ));
        }
        if !git::ref_is_stable(root, &options.base, &plan.target_sha)? {
            return Err(CrivError::new(format!(
                "target ref `{}` moved before the reconciliation commit; retry against its new SHA",
                options.base
            )));
        }
        git::commit_staged(root, RECONCILIATION_COMMIT_MESSAGE)
    })();
    let commit = match commit {
        Ok(commit) => commit,
        Err(error) => {
            let rollback_errors = snapshot.rollback();
            return Err(if rollback_errors.is_empty() {
                error
            } else {
                CrivError::new(format!(
                    "{error}\nADR reconciliation rollback also failed:\n{}",
                    rollback_errors.join("\n")
                ))
            });
        }
    };
    println!("ADR reconciliation committed: {commit}");
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "the reconciliation plan validates one complete Git and vault snapshot"
)]
fn build_plan_from(
    files: &RepositoryFiles,
    base_ref: &str,
    target_sha: &str,
) -> Result<ReconcilePlan> {
    let root = files.root();
    let vault = Vault::load_from(files)?;
    let current_config = &vault.config;
    let target_config = if git::tree_paths(root, target_sha, "criv.toml")?
        .iter()
        .any(|path| path == "criv.toml")
    {
        Some(git::blob(root, target_sha, "criv.toml")?)
    } else {
        None
    };
    let target_config = Config::parse(target_config.as_deref())?;
    if current_config.docs_dir != target_config.docs_dir
        || current_config.adr_dir != target_config.adr_dir
    {
        return Err(CrivError::new(
            "refusing ADR reconciliation because vault.docs or vault.adr differs from the target; cannot prove ADR ownership",
        ));
    }
    let adr_prefix = format!("{}/{}/", vault.config.docs_dir, vault.config.adr_dir);
    let merge_base = git::merge_base(root, target_sha, "HEAD")?;
    let target_paths = git::tree_paths(root, target_sha, &adr_prefix)?;
    let merge_paths = git::tree_paths(root, &merge_base, &adr_prefix)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let target = target_paths
        .iter()
        .filter(|path| is_adr_path(&adr_prefix, path))
        .map(|path| {
            git::blob(root, target_sha, path).and_then(|contents| parse_adr(path, contents))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure_unique(&target, "target")?;
    let target_by_id = target
        .iter()
        .map(|adr| (adr.id, adr))
        .collect::<BTreeMap<_, _>>();
    let target_paths = target_paths.into_iter().collect::<BTreeSet<_>>();

    let materialized = current_materialized_receipt(root)?;
    let prior_mappings = materialized
        .as_ref()
        .map(|receipt| &receipt.mappings)
        .into_iter()
        .flatten()
        .map(|mapping| (mapping.old_path.clone(), mapping.clone()))
        .collect::<BTreeMap<_, _>>();
    let current_paths = materialized
        .as_ref()
        .map(|receipt| {
            receipt
                .mappings
                .iter()
                .map(|mapping| (mapping.old_path.clone(), mapping.new_path.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let history_changes = git::changes_between(root, &merge_base, "HEAD")?;
    let changes = git::changes_between_paths(root, &merge_base, "HEAD", &[&adr_prefix])?;
    let worktree_changes = git::worktree_changes_in(root, &[&adr_prefix])?;
    let worktree_moves = worktree_changes
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.status,
                git::ChangeStatus::Renamed | git::ChangeStatus::Copied
            )
        })
        .cloned()
        .filter_map(|entry| entry.previous_path.map(|previous| (previous, entry.path)))
        .collect::<BTreeMap<_, _>>();
    let worktree_additions = worktree_changes
        .entries
        .iter()
        .filter(|entry| entry.status == git::ChangeStatus::Added)
        .filter(|entry| is_adr_path(&adr_prefix, &entry.path))
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let mut receipt_sources = Vec::new();
    let mut branch_adrs = changes
        .entries
        .iter()
        .filter(|entry| is_adr_path(&adr_prefix, &entry.path))
        .filter(|entry| {
            !source_reconcile::allows_history_change(root, &history_changes, entry)
        })
        .map(|entry| match entry.status {
            git::ChangeStatus::Added => {
                let current_path = current_paths.get(&entry.path).cloned().or_else(|| worktree_moves.get(&entry.path).cloned()).or_else(|| {
                    let mut candidates = worktree_additions
                        .iter()
                        .filter(|candidate| same_adr_slug(&entry.path, candidate));
                    let candidate = candidates.next()?.clone();
                    candidates.next().is_none().then_some(candidate)
                });
                let current_path = current_path.as_deref().unwrap_or(&entry.path);
                let contents = files.read_string(Path::new(current_path)).map_err(|error| {
                    CrivError::new(format!(
                        "cannot read branch-created ADR `{current_path}` while proving ownership: {error}"
                    ))
                })?;
                ensure_proven_new_adr(
                    root,
                    &entry.path,
                    &contents,
                    &merge_base,
                    target_sha,
                    &merge_paths,
                    &target_paths,
                )?;
                let adr = parse_adr(current_path, contents)?;
                let source_contents = git::blob(root, "HEAD", &entry.path)?;
                receipt_sources.push(parse_adr(&entry.path, source_contents)?);
                Ok(adr)
            }
            git::ChangeStatus::Renamed | git::ChangeStatus::Copied => Err(CrivError::new(format!(
                "ADR `{}` was renamed or copied from `{}`; published ADR content is immutable",
                entry.path,
                entry
                    .previous_path
                    .as_deref()
                    .unwrap_or("an inherited path")
            ))),
            _ => Err(CrivError::new(format!(
                "ADR `{}` is not a branch-created addition; published ADR content is immutable",
                entry.path
            ))),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut branch_paths = branch_adrs
        .iter()
        .map(|adr| adr.path.clone())
        .collect::<BTreeSet<_>>();
    for entry in &worktree_changes.entries {
        if !is_adr_path(&adr_prefix, &entry.path) {
            continue;
        }
        match entry.status {
            git::ChangeStatus::Added if branch_paths.insert(entry.path.clone()) => {
                let contents = files.read_string(Path::new(&entry.path))?;
                ensure_proven_new_adr(
                    root,
                    &entry.path,
                    &contents,
                    &merge_base,
                    target_sha,
                    &merge_paths,
                    &target_paths,
                )?;
                branch_adrs.push(parse_adr(&entry.path, contents)?);
            }
            git::ChangeStatus::Renamed | git::ChangeStatus::Copied
                if entry
                    .previous_path
                    .as_deref()
                    .is_some_and(|path| merge_paths.contains(path)) =>
            {
                return Err(CrivError::new(format!(
                    "ADR `{}` was renamed or copied from published ADR `{}`; published ADR content is immutable",
                    entry.path,
                    entry.previous_path.as_deref().unwrap_or_default()
                )));
            }
            _ => {}
        }
    }
    ensure_unique(&branch_adrs, "branch-local")?;

    let target_max = target.iter().map(|adr| adr.id).max().unwrap_or(0);
    let conflicted = branch_adrs.iter().any(|adr| {
        target_paths.contains(&adr.path)
            || target_by_id
                .get(&adr.id)
                .is_some_and(|target| target.path != adr.path)
            || adr.id <= target_max
    });
    let mappings = allocation_mappings(&branch_adrs, target_max, conflicted)?;
    let receipt_mappings = mappings
        .iter()
        .map(|mapping| {
            let original = prior_mappings
                .values()
                .find(|prior| prior.new_path == mapping.old_path)
                .unwrap_or(mapping);
            Mapping {
                old_id: original.old_id,
                new_id: mapping.new_id,
                old_path: original.old_path.clone(),
                new_path: mapping.new_path.clone(),
            }
        })
        .collect::<Vec<_>>();
    let destination_paths = mappings
        .iter()
        .map(|mapping| mapping.new_path.clone())
        .collect::<BTreeSet<_>>();
    if destination_paths.len() != mappings.len() {
        return Err(CrivError::new("ADR reconciliation destinations collide"));
    }
    for mapping in &mappings {
        if mapping.old_path != mapping.new_path
            && root.join(&mapping.new_path).exists()
            && !mappings
                .iter()
                .any(|other| other.old_path == mapping.new_path)
        {
            return Err(CrivError::new(format!(
                "ADR reconciliation destination `{}` already exists",
                mapping.new_path
            )));
        }
    }
    Ok(ReconcilePlan {
        base_ref: base_ref.into(),
        head_sha: git::resolve_commit(root, "HEAD")?,
        target_sha: target_sha.into(),
        merge_base,
        mappings,
        receipt_mappings,
        branch_adrs,
        receipt_sources,
    })
}

/// Git's `A` status is not proof of authorship: an inherited ADR can be
/// copied and changed enough to miss its normal rename threshold. Restrict
/// candidate discovery to the ADR vault, then reject a new-looking file when
/// it retains distinctive published material.
fn ensure_proven_new_adr(
    root: &Path,
    path: &str,
    contents: &str,
    merge_base: &str,
    target_sha: &str,
    merge_paths: &BTreeSet<String>,
    target_paths: &BTreeSet<String>,
) -> Result<()> {
    let candidates = merge_paths
        .iter()
        .map(|path| (merge_base, path))
        .chain(target_paths.iter().map(|path| (target_sha, path)));
    for (revision, candidate) in candidates {
        if candidate == path
            || !Path::new(candidate)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let source = git::blob(root, revision, candidate)?;
        if plausible_carried_content(contents, &source) {
            return Err(CrivError::new(format!(
                "ADR `{path}` appears to carry published content from `{candidate}`; copied or renamed ADRs are immutable"
            )));
        }
    }
    Ok(())
}

/// Compare ADR meaning rather than its standard template. Identity and
/// governance metadata, the rendered title, and conventional section headings
/// are scaffolding shared by independent decisions and cannot prove copying.
fn plausible_carried_content(first: &str, second: &str) -> bool {
    if hash(first) == hash(second) {
        return true;
    }
    let first = adr_semantic_content(first);
    let second = adr_semantic_content(second);
    let shared_body = first.body.intersection(&second.body).collect::<Vec<_>>();

    if first.title.is_some() && first.title == second.title && !shared_body.is_empty() {
        return true;
    }
    if first.body == second.body && !first.body.is_empty() {
        return first.body.len() > 1 || first.body.iter().any(|line| line.len() >= 24);
    }
    let shared_chars = shared_body.iter().map(|line| line.len()).sum::<usize>();
    let smaller_body_chars = first
        .body
        .iter()
        .map(std::string::String::len)
        .sum::<usize>()
        .min(
            second
                .body
                .iter()
                .map(std::string::String::len)
                .sum::<usize>(),
        );
    shared_chars >= 80 && shared_chars.saturating_mul(2) >= smaller_body_chars
}

#[derive(Debug, Eq, PartialEq)]
struct AdrSemanticContent {
    title: Option<String>,
    body: BTreeSet<String>,
}

fn adr_semantic_content(contents: &str) -> AdrSemanticContent {
    let mut in_frontmatter = false;
    let mut in_code_fence = false;
    let mut title = None;
    let mut body = BTreeSet::new();
    for line in contents.lines() {
        let line = line.trim();
        if !in_frontmatter && (line.starts_with("```") || line.starts_with("~~~")) {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }
        if line == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter {
            if let Some(value) = line.strip_prefix("title:") {
                title = Some(value.trim().trim_matches(['\'', '"']).to_owned());
            }
            continue;
        }
        if line.is_empty()
            || line.starts_with("# ")
            || line
                .strip_prefix("## ")
                .is_some_and(is_standard_adr_heading)
        {
            continue;
        }
        body.insert(line.to_owned());
    }
    AdrSemanticContent { title, body }
}

fn is_standard_adr_heading(heading: &str) -> bool {
    matches!(
        heading.trim().to_ascii_lowercase().as_str(),
        "context"
            | "status"
            | "decision"
            | "consequences"
            | "alternatives considered"
            | "decision drivers"
            | "considered options"
            | "pros and cons of the options"
            | "links"
    )
}

fn allocation_mappings(
    branch_adrs: &[AdrFile],
    target_max: u32,
    conflicted: bool,
) -> Result<Vec<Mapping>> {
    if !conflicted {
        return Ok(Vec::new());
    }
    let mut ordered = branch_adrs.to_vec();
    ordered.sort_by_key(|adr| adr.id);
    let allocation_end = target_max
        .checked_add(
            u32::try_from(ordered.len())
                .map_err(|_| CrivError::new("ADR ID allocation count exceeds u32"))?,
        )
        .ok_or_else(|| CrivError::new("ADR ID allocation overflow"))?;
    if allocation_end > 9999 {
        return Err(CrivError::new(
            "ADR reconciliation allocation exceeds ADR-9999",
        ));
    }
    ordered
        .into_iter()
        .enumerate()
        .map(|(offset, adr)| {
            let new_id = target_max
                .checked_add(
                    u32::try_from(offset)
                        .map_err(|_| CrivError::new("ADR ID allocation offset exceeds u32"))?
                        .saturating_add(1),
                )
                .ok_or_else(|| CrivError::new("ADR ID allocation overflow"))?;
            let new_name = filename(new_id, &adr.slug);
            let parent = Path::new(&adr.path)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            Ok(Mapping {
                old_id: adr.id,
                new_id,
                old_path: adr.path.clone(),
                new_path: parent.join(new_name).to_string_lossy().replace('\\', "/"),
            })
        })
        .collect()
}

fn is_adr_path(prefix: &str, path: &str) -> bool {
    path.starts_with(prefix)
        && path != format!("{prefix}README.md")
        && Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn same_adr_slug(first: &str, second: &str) -> bool {
    [first, second]
        .into_iter()
        .map(|path| {
            Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.split_once('-').map(|(_, slug)| slug))
        })
        .collect::<Option<Vec<_>>>()
        .is_some_and(|slugs| {
            slugs
                .first()
                .zip(slugs.get(1))
                .is_some_and(|(first, second)| first == second)
        })
}

fn parse_adr(path: &str, contents: String) -> Result<AdrFile> {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CrivError::new(format!("ADR path `{path}` is not valid UTF-8")))?;
    let stem = name
        .strip_suffix(".md")
        .ok_or_else(|| CrivError::new(format!("ADR `{path}` must be Markdown")))?;
    let (number, slug) = stem.split_once('-').ok_or_else(|| {
        CrivError::new(format!(
            "branch-local ADR `{path}` must use NNNN-kebab-title.md"
        ))
    })?;
    if number.len() != 4 || !number.bytes().all(|byte| byte.is_ascii_digit()) || slug.is_empty() {
        return Err(CrivError::new(format!(
            "branch-local ADR `{path}` has a malformed filename"
        )));
    }
    let id = number
        .parse::<u32>()
        .map_err(|error| CrivError::new(format!("ADR `{path}` has an invalid ID: {error}")))?;
    let normalized = contents.replace("\r\n", "\n");
    let frontmatter = normalized
        .strip_prefix("---\n")
        .and_then(|rest| {
            rest.split_once("\n---\n")
                .map(|(frontmatter, _)| frontmatter)
        })
        .ok_or_else(|| {
            CrivError::new(format!("branch-local ADR `{path}` has no YAML frontmatter"))
        })?;
    let expected_id = format!("ADR-{number}");
    let actual_id = frontmatter
        .lines()
        .find_map(|line| line.strip_prefix("id:").map(str::trim));
    if actual_id != Some(expected_id.as_str()) {
        return Err(CrivError::new(format!(
            "ADR `{path}` filename and frontmatter id must agree"
        )));
    }
    Ok(AdrFile {
        path: path.into(),
        id,
        slug: slug.into(),
        contents,
    })
}

fn ensure_unique(adrs: &[AdrFile], owner: &str) -> Result<()> {
    let mut ids = BTreeSet::new();
    for adr in adrs {
        if !ids.insert(adr.id) {
            return Err(CrivError::new(format!(
                "duplicate {owner} ADR ID ADR-{:04}",
                adr.id
            )));
        }
    }
    Ok(())
}

fn filename(id: u32, slug: &str) -> String {
    format!("{id:04}-{slug}.md")
}

fn print_mapping(mappings: &[Mapping]) {
    println!("ADR reconciliation mapping:");
    for mapping in mappings {
        println!(
            "  ADR-{:04} -> ADR-{:04} ({})",
            mapping.old_id, mapping.new_id, mapping.new_path
        );
    }
}

fn transaction_paths(root: &Path, plan: &ReconcilePlan) -> Result<Vec<String>> {
    let mut paths = rewrite_candidates(root, plan)?
        .into_keys()
        .collect::<BTreeSet<_>>();
    paths.extend(
        superseded_paths(&plan.mappings)
            .into_iter()
            .chain(superseded_paths(&plan.receipt_mappings))
            .map(str::to_string),
    );
    Ok(paths.into_iter().collect())
}

fn receipt_paths(receipt: &Receipt) -> Vec<String> {
    receipt
        .files
        .iter()
        .map(|file| file.path.clone())
        .chain(receipt.deletions.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn apply_plan(files: &RepositoryFiles, plan: &ReconcilePlan) -> Result<()> {
    let root = files.root();
    let rewrite_paths = rewrite_candidates(root, plan)?;
    // Capture every source before publishing any destination. A destination
    // may be another mapping's source, so reading permissions inside the write
    // loop would make the result depend on mapping order.
    let destination_permissions = plan
        .mappings
        .iter()
        .map(|mapping| {
            Ok((
                mapping.new_path.clone(),
                files.permissions(Path::new(&mapping.old_path))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut receipt_files = Vec::new();
    for (path, contents) in &rewrite_paths {
        if let Some(permissions) = destination_permissions.get(path) {
            files
                .write_scope(Path::new("."))?
                .write_atomic_with_permissions(Path::new(path), contents, permissions.clone())?;
        } else {
            files
                .write_scope(Path::new("."))?
                .write_atomic(Path::new(path), contents)?;
        }
        receipt_files.push(ReceiptFile {
            path: path.clone(),
            before_hash: git::blob(root, &plan.head_sha, path)
                .ok()
                .map(|before| hash(&before)),
            after_hash: hash(&git::worktree_blob(root, path)?),
            before_mode: git::file_mode(root, &plan.head_sha, path)?,
            after_mode: worktree_file_mode(root, path)?,
        });
    }
    let deletions = superseded_paths(&plan.receipt_mappings)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let physical_deletions = superseded_paths(&plan.mappings)
        .into_iter()
        .chain(deletions.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    for path in physical_deletions {
        // A retry expresses its final transaction against the original HEAD;
        // an earlier materialized receipt may already have removed this path.
        if files.file_exists(Path::new(path))? {
            files
                .write_scope(Path::new("."))?
                .remove_file(Path::new(path))?;
        }
    }
    let errors = crate::check::validate_all_from(files)?
        .into_iter()
        .filter(super::check::Diagnostic::is_error)
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(CrivError::new(format!(
            "reconciliation wrote files but vault validation failed:\n{}",
            errors
                .into_iter()
                .map(|error| error.describe())
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.into(),
        base_ref: plan.base_ref.clone(),
        head_sha: plan.head_sha.clone(),
        target_sha: plan.target_sha.clone(),
        merge_base: plan.merge_base.clone(),
        mappings: plan.receipt_mappings.clone(),
        sources: plan
            .receipt_sources
            .iter()
            .map(|adr| {
                Ok(ReceiptSource {
                    path: adr.path.clone(),
                    before_hash: hash(&adr.contents),
                    before_mode: git::file_mode(root, &plan.head_sha, &adr.path)?.ok_or_else(
                        || CrivError::new(format!("cannot read Git mode for `{}`", adr.path)),
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        deletions,
        files: receipt_files,
    };
    let contents = serde_json::to_string_pretty(&receipt).map_err(|error| {
        CrivError::new(format!("cannot serialize reconciliation receipt: {error}"))
    })? + "\n";
    files
        .write_scope(Path::new(".criv"))?
        .write_atomic(Path::new(".criv/adr-reconcile.json"), &contents)
}

/// A generated reconcile operation is safe to re-plan before it is staged or
/// committed only when every changed path is exactly the receipt's output.
/// This intentionally does not inspect the target ref: an advanced target is
/// the reason a retry may be needed.
fn materialized_receipt(root: &Path) -> Result<Receipt> {
    let receipt = read_receipt(root)?;
    if receipt.schema != RECEIPT_SCHEMA
        || git::resolve_commit(root, "HEAD")? != receipt.head_sha
        || receipt.sources.iter().any(|source| {
            git::blob(root, &receipt.head_sha, &source.path)
                .map_or(true, |contents| hash(&contents) != source.before_hash)
                || git::file_mode(root, &receipt.head_sha, &source.path)
                    .ok()
                    .flatten()
                    .as_deref()
                    != Some(source.before_mode.as_str())
        })
        || receipt.files.iter().any(|file| {
            let before_matches = match &file.before_hash {
                Some(before_hash) => git::blob(root, &receipt.head_sha, &file.path)
                    .is_ok_and(|contents| hash(&contents) == *before_hash),
                None => git::blob(root, &receipt.head_sha, &file.path).is_err(),
            };
            !before_matches
                || git::file_mode(root, &receipt.head_sha, &file.path)
                    .ok()
                    .flatten()
                    .as_deref()
                    != file.before_mode.as_deref()
                || git::worktree_blob(root, &file.path)
                    .map_or(true, |contents| hash(&contents) != file.after_hash)
                || worktree_file_mode(root, &file.path).map_or(true, |mode| mode != file.after_mode)
        })
        || receipt
            .deletions
            .iter()
            .any(|path| root.join(path).exists())
    {
        return Err(CrivError::new(
            "ADR reconciliation receipt does not match the materialized worktree",
        ));
    }
    let expected = receipt
        .files
        .iter()
        .map(|file| file.path.clone())
        .chain(receipt.deletions.iter().cloned())
        .collect::<BTreeSet<_>>();
    let actual = git::dirty_paths(root)?.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(CrivError::new(format!(
            "ADR reconciliation receipt does not cover every dirty worktree path (expected: {}; actual: {})",
            expected.iter().cloned().collect::<Vec<_>>().join(", "),
            actual.iter().cloned().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(receipt)
}

/// Old receipts deliberately become inert after their planning commit is no
/// longer HEAD. A receipt that claims to describe the current HEAD, however,
/// must be valid rather than silently bypassed.
fn current_materialized_receipt(root: &Path) -> Result<Option<Receipt>> {
    let receipt_path = root.join(".criv/adr-reconcile.json");
    if !receipt_path.exists() {
        return Ok(None);
    }
    let receipt = read_receipt(root)?;
    if git::resolve_commit(root, "HEAD")? != receipt.head_sha {
        return Ok(None);
    }
    materialized_receipt(root).map(Some)
}

/// A destination can also be another mapping's source. In that case its old
/// contents have already been copied to the later destination, and removing
/// the source would erase the newly published earlier ADR.
fn superseded_paths(mappings: &[Mapping]) -> Vec<&str> {
    let destinations = mappings
        .iter()
        .map(|mapping| mapping.new_path.as_str())
        .collect::<BTreeSet<_>>();
    mappings
        .iter()
        .filter(|mapping| {
            mapping.old_path != mapping.new_path
                && !destinations.contains(mapping.old_path.as_str())
        })
        .map(|mapping| mapping.old_path.as_str())
        .collect()
}

#[expect(
    clippy::too_many_lines,
    reason = "candidate rewriting keeps file safety checks with their rewrite decisions"
)]
fn rewrite_candidates(root: &Path, plan: &ReconcilePlan) -> Result<BTreeMap<String, String>> {
    let changed_paths = git::changes_between(root, &plan.merge_base, "HEAD")?
        .entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut paths = git::tree_paths(root, "HEAD", ".")?;
    for adr in &plan.branch_adrs {
        if !paths.contains(&adr.path) {
            paths.push(adr.path.clone());
        }
    }
    let files = RepositoryFiles::open(root)?;
    let mut rewrites = BTreeMap::new();
    let mut inherited_references = None;
    for path in paths {
        if path == ".criv/adr-reconcile.json" || path.starts_with(".git/") {
            continue;
        }
        if !files.file_exists(Path::new(&path))? {
            continue;
        }
        let Ok(contents) = files.read_string(Path::new(&path)) else {
            let bytes = files.read(Path::new(&path))?;
            if contains_reference(&bytes, &plan.mappings) {
                return Err(CrivError::new(format!(
                    "refusing to reconcile binary or non-UTF-8 file `{path}` containing an ADR reference"
                )));
            }
            continue;
        };
        let rewritten = if plan.branch_adrs.iter().any(|adr| adr.path == path) {
            // A receipt can have already moved this branch-owned ADR to an
            // untracked path, so it has no `HEAD` diff entry to attribute.
            rewrite_text(&contents, &plan.mappings)?
        } else {
            match changed_paths.get(&path) {
                Some(entry) if entry.status == git::ChangeStatus::Added => {
                    if reference_edits(&contents, &plan.mappings)?.is_empty() {
                        contents.clone()
                    } else {
                        let evidence = match &inherited_references {
                            Some(evidence) => evidence,
                            None => inherited_references.insert(inherited_reference_sources(
                                root,
                                &plan.merge_base,
                                &plan.mappings,
                            )?),
                        };
                        match inherited_reference_source(
                            &path,
                            &contents,
                            evidence,
                            &plan.mappings,
                        )? {
                            Some(source) => rewrite_owned_lines(
                                root,
                                &contents,
                                &plan.merge_base,
                                &source,
                                &path,
                                &plan.mappings,
                            )?,
                            None => rewrite_text(&contents, &plan.mappings)?,
                        }
                    }
                }
                Some(entry)
                    if matches!(
                        entry.status,
                        git::ChangeStatus::Renamed | git::ChangeStatus::Copied
                    ) =>
                {
                    rewrite_owned_lines(
                        root,
                        &contents,
                        &plan.merge_base,
                        entry.previous_path.as_deref().ok_or_else(|| {
                            CrivError::new(format!(
                                "Git did not report an inherited path for `{path}`"
                            ))
                        })?,
                        &path,
                        &plan.mappings,
                    )?
                }
                _ => rewrite_owned_lines(
                    root,
                    &contents,
                    &plan.merge_base,
                    &path,
                    &path,
                    &plan.mappings,
                )?,
            }
        };
        let output_path = plan
            .mappings
            .iter()
            .find(|mapping| mapping.old_path == path)
            .map_or_else(|| path.clone(), |mapping| mapping.new_path.clone());
        if rewritten != contents || output_path != path {
            rewrites.insert(output_path, rewritten);
        }
    }
    for adr in &plan.branch_adrs {
        if let Some(mapping) = plan
            .mappings
            .iter()
            .find(|mapping| mapping.old_path == adr.path)
            && !rewrites.contains_key(&mapping.new_path)
        {
            rewrites.insert(
                mapping.new_path.clone(),
                rewrite_text(&adr.contents, &plan.mappings)?,
            );
        }
    }
    Ok(rewrites)
}

/// Index exact reference-bearing lines from the merge base once. Only those
/// lines are relevant to rewrite ownership; unrelated copied text is not.
fn inherited_reference_sources(
    root: &Path,
    merge_base: &str,
    mappings: &[Mapping],
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut evidence = BTreeMap::<String, BTreeSet<String>>::new();
    for candidate in git::tree_paths(root, merge_base, ".")? {
        let Ok(contents) = git::blob(root, merge_base, &candidate) else {
            continue;
        };
        for line in contents.split_inclusive('\n') {
            if contains_reference(line.as_bytes(), mappings) {
                evidence
                    .entry(git_comparison_line(line).into_owned())
                    .or_default()
                    .insert(candidate.clone());
            }
        }
    }
    Ok(evidence)
}

fn inherited_reference_source(
    path: &str,
    contents: &str,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    mappings: &[Mapping],
) -> Result<Option<String>> {
    let sources = contents
        .split_inclusive('\n')
        .filter(|line| contains_reference(line.as_bytes(), mappings))
        .filter_map(|line| {
            let line = git_comparison_line(line);
            evidence.get(line.as_ref())
        })
        .flatten()
        .filter(|candidate| candidate.as_str() != path)
        .cloned()
        .collect::<BTreeSet<_>>();
    if sources.len() > 1 {
        return Err(CrivError::new(format!(
            "file `{path}` has ambiguous inherited provenance; cannot prove its ADR references are branch-owned"
        )));
    }
    Ok(sources.into_iter().next())
}

fn git_comparison_line(line: &str) -> Cow<'_, str> {
    line.strip_suffix("\r\n").map_or_else(
        || Cow::Borrowed(line),
        |line| Cow::Owned(format!("{line}\n")),
    )
}

fn rewrite_owned_lines(
    root: &Path,
    contents: &str,
    merge_base: &str,
    original_path: &str,
    path: &str,
    mappings: &[Mapping],
) -> Result<String> {
    let ranges = if original_path == path {
        git::added_lines(root, merge_base, "HEAD", path)?
    } else {
        git::added_lines_between_blobs(root, merge_base, original_path, "HEAD", path)?
    };
    let mut output = String::new();
    for (index, line) in contents.split_inclusive('\n').enumerate() {
        let line_number = index.saturating_add(1);
        let owned = ranges.iter().any(|range| range.contains(&line_number));
        let rewritten = if owned {
            rewrite_text(line, mappings)?
        } else {
            line.to_owned()
        };
        if !owned && contains_reference(line.as_bytes(), mappings) {
            return Err(CrivError::new(format!(
                "refusing to rewrite target-owned reference in `{path}` line {line_number}"
            )));
        }
        output.push_str(&rewritten);
    }
    Ok(output)
}

fn contains_reference(bytes: &[u8], mappings: &[Mapping]) -> bool {
    std::str::from_utf8(bytes).ok().is_some_and(|text| {
        !reference_edits(text, mappings)
            .unwrap_or_default()
            .is_empty()
    }) || mappings
        .iter()
        .any(|mapping| contains_exact_id(bytes, mapping.old_id))
        || contains_wikilink_reference_bytes(bytes, mappings)
}

#[derive(Debug, Eq, PartialEq)]
struct TextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

fn rewrite_text(contents: &str, mappings: &[Mapping]) -> Result<String> {
    let edits = reference_edits(contents, mappings)?;
    let mut output = contents.to_owned();
    for edit in edits.into_iter().rev() {
        if output.get(edit.start..edit.end).is_some() {
            output.replace_range(edit.start..edit.end, &edit.replacement);
        }
    }
    Ok(output)
}

fn reference_edits(contents: &str, mappings: &[Mapping]) -> Result<Vec<TextEdit>> {
    let bytes = contents.as_bytes();
    let mut edits = Vec::new();
    for mapping in mappings {
        let old_id = format!("ADR-{:04}", mapping.old_id);
        let new_id = format!("ADR-{:04}", mapping.new_id);
        for (start, _) in contents.match_indices(&old_id) {
            let Some(end) = start.checked_add(old_id.len()) else {
                continue;
            };
            if is_exact_token(bytes, start, end) {
                edits.push(TextEdit {
                    start,
                    end,
                    replacement: new_id.clone(),
                });
            }
        }
    }
    let mut cursor = 0;
    while let Some(tail) = contents.get(cursor..) {
        let Some(relative_start) = tail.find("[[") else {
            break;
        };
        let Some(start) = cursor.checked_add(relative_start) else {
            break;
        };
        let Some(body_start) = start.checked_add(2) else {
            break;
        };
        let Some(body_tail) = contents.get(body_start..) else {
            break;
        };
        let Some(relative_end) = body_tail.find("]]") else {
            break;
        };
        let Some(end) = body_start.checked_add(relative_end) else {
            break;
        };
        add_wikilink_edits(contents, body_start, end, mappings, &mut edits);
        let Some(next) = end.checked_add(2) else {
            break;
        };
        cursor = next;
    }
    edits.sort_by_key(|edit| (edit.start, edit.end));
    for pair in edits.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        let overlaps = left
            .end
            .checked_sub(right.start)
            .is_some_and(|distance| distance > 0);
        if overlaps
            && (left.start != right.start
                || left.end != right.end
                || left.replacement != right.replacement)
        {
            return Err(CrivError::new("ADR reference edits overlap ambiguously"));
        }
    }
    edits.dedup();
    Ok(edits)
}

fn add_wikilink_edits(
    contents: &str,
    start: usize,
    end: usize,
    mappings: &[Mapping],
    edits: &mut Vec<TextEdit>,
) {
    let Some(body) = contents.get(start..end) else {
        return;
    };
    let mut part_start = start;
    for part in body.split('|') {
        let link_target = part.split_once('#').map_or(part, |(target, _)| target);
        for mapping in mappings {
            let old_name = Path::new(&mapping.old_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&mapping.old_path);
            let old_stem = old_name.strip_suffix(".md").unwrap_or(old_name);
            let new_name = Path::new(&mapping.new_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&mapping.new_path);
            let new_stem = new_name.strip_suffix(".md").unwrap_or(new_name);
            let replacement = if link_target == mapping.old_path {
                Some(mapping.new_path.as_str())
            } else if link_target == old_name {
                Some(new_name)
            } else if link_target == old_stem {
                Some(new_stem)
            } else {
                None
            };
            if let Some(replacement) = replacement {
                edits.push(TextEdit {
                    start: part_start,
                    end: part_start.saturating_add(link_target.len()),
                    replacement: replacement.to_owned(),
                });
            }
        }
        part_start = part_start.saturating_add(part.len()).saturating_add(1);
    }
}

fn contains_exact_id(bytes: &[u8], id: u32) -> bool {
    let needle = format!("ADR-{id:04}");
    bytes
        .windows(needle.len())
        .enumerate()
        .any(|(start, window)| {
            window == needle.as_bytes()
                && start
                    .checked_add(needle.len())
                    .is_some_and(|end| is_exact_token(bytes, start, end))
        })
}

fn contains_wikilink_reference_bytes(bytes: &[u8], mappings: &[Mapping]) -> bool {
    let mut cursor = 0;
    while let Some(tail) = bytes.get(cursor..) {
        let Some(relative_start) = tail.windows(2).position(|window| window == b"[[") else {
            break;
        };
        let Some(start) = cursor
            .checked_add(relative_start)
            .and_then(|offset| offset.checked_add(2))
        else {
            break;
        };
        let Some(relative_end) = bytes
            .get(start..)
            .and_then(|tail| tail.windows(2).position(|window| window == b"]]"))
        else {
            break;
        };
        let Some(end) = start.checked_add(relative_end) else {
            break;
        };
        let Some(body) = bytes.get(start..end) else {
            break;
        };
        if body.split(|byte| *byte == b'|').any(|part| {
            let target = part.split(|byte| *byte == b'#').next().unwrap_or(part);
            mappings.iter().any(|mapping| {
                let name = Path::new(&mapping.old_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&mapping.old_path);
                let stem = name.strip_suffix(".md").unwrap_or(name);
                target == mapping.old_path.as_bytes()
                    || target == name.as_bytes()
                    || target == stem.as_bytes()
            })
        }) {
            return true;
        }
        let Some(next) = end.checked_add(2) else {
            break;
        };
        cursor = next;
    }
    false
}

fn is_exact_token(bytes: &[u8], start: usize, end: usize) -> bool {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let before_is_word = start
        .checked_sub(1)
        .and_then(|index| bytes.get(index))
        .is_some_and(|byte| is_word(*byte));
    let after_is_word = bytes.get(end).is_some_and(|byte| is_word(*byte));
    !before_is_word && !after_is_word
}

fn hash(contents: &str) -> String {
    blake3::hash(contents.as_bytes()).to_hex().to_string()
}

fn worktree_file_mode(root: &Path, path: &str) -> Result<String> {
    let metadata = fs::symlink_metadata(root.join(path))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CrivError::new(format!(
            "cannot record Git mode for non-file `{path}`"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(if metadata.permissions().mode() & 0o100 == 0 {
            "100644".into()
        } else {
            "100755".into()
        })
    }
    #[cfg(not(unix))]
    {
        Ok("100644".into())
    }
}

fn read_receipt(root: &Path) -> Result<Receipt> {
    let files = RepositoryFiles::open(root)?;
    let receipt: Receipt =
        serde_json::from_str(&files.read_string(Path::new(RECEIPT_PATH))?).map_err(|_| {
        CrivError::new(
            "ADR reconciliation receipt is malformed; remove it only after recovering the worktree",
        )
    })?;
    if receipt.schema != RECEIPT_SCHEMA {
        return Err(CrivError::new(format!(
            "ADR reconciliation receipt schema `{}` is unsupported; expected `{RECEIPT_SCHEMA}`",
            receipt.schema
        )));
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a committed ADR fixture so receipt tests compare real Git trees.
    fn decision_repository() -> tempfile::TempDir {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init", "-b", "main"]);
        git(root.path(), &["config", "user.email", "criv@example.com"]);
        git(root.path(), &["config", "user.name", "criv"]);
        fs::create_dir_all(root.path().join("docs/adr")).unwrap();
        fs::write(
            root.path().join("criv.toml"),
            "[vault]\ndocs = \"docs\"\nadr = \"adr\"\n",
        )
        .unwrap();
        fs::write(root.path().join("docs/adr/0001-base.md"), "Base decision\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "base"]);
        root
    }

    #[test]
    /// Keep file errors before receipt errors and report scope changes last.
    fn current_receipt_failures_follow_file_order_and_precede_scope_failure() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let head = git::resolve_commit(root.path(), "HEAD").unwrap();
        fs::create_dir_all(root.path().join(".criv")).unwrap();
        let mut receipt = serde_json::json!({
            "schema": RECEIPT_SCHEMA, "base_ref": "missing", "target_sha": head,
            "head_sha": head, "merge_base": head, "mappings": [], "sources": [],
            "deletions": [], "files": []
        });
        fs::write(root.path().join(RECEIPT_PATH), receipt.to_string()).unwrap();
        receipt["schema"] = "criv.source-reconcile/1".into();
        fs::write(
            root.path().join(".criv/source-reconcile.json"),
            receipt.to_string(),
        )
        .unwrap();
        fs::write(root.path().join("docs/adr/0001-base.md"), "Edited\n").unwrap();
        fs::write(root.path().join("criv.toml"), "# Changed scope\n").unwrap();
        let mut changes =
            git::worktree_changes_in(root.path(), &["docs/adr", "criv.toml"]).unwrap();
        changes.entries.push(ChangedEntry {
            status: ChangeStatus::Modified,
            path: "other/adr/0002-existing.md".into(),
            previous_path: None,
            old_ref: Some("HEAD".into()),
            new_ref: None,
        });
        let config = Config {
            docs_dir: "other".into(),
            ..Config::default()
        };
        for mode in [ChangeMode::Commit, ChangeMode::Push] {
            let violations = change_violations(&files, &config, Some(&changes), mode);
            assert_eq!(violations.len(), 4);
            assert!(violations[0].starts_with("other/adr/0002-existing.md:"));
            assert_eq!(
                violations[1],
                "ADR reconciliation receipt does not prove the complete staged transaction"
            );
            assert_eq!(
                violations[2],
                "source reconciliation receipt does not prove the complete staged transaction"
            );
            assert!(violations[3].starts_with("criv.toml moves the decision scope"));
        }
        let violations = change_violations(&files, &config, None, ChangeMode::Commit);
        assert_eq!(violations.len(), 2);
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "advance beyond receipts"]);
        assert!(change_violations(&files, &config, None, ChangeMode::Commit).is_empty());
    }

    #[test]
    /// Require a valid complete transaction before a receipt grants permission.
    fn absent_comparison_and_invalid_receipt_do_not_prove_changes() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        fs::create_dir_all(root.path().join(".criv")).unwrap();
        fs::write(root.path().join(RECEIPT_PATH), "invalid receipt").unwrap();
        for mode in [ChangeMode::Commit, ChangeMode::Push, ChangeMode::Ci] {
            assert!(change_violations(&files, &Config::default(), None, mode).is_empty());
        }
        fs::write(
            root.path().join("docs/adr/0001-base.md"),
            "Changed decision\n",
        )
        .unwrap();
        let changes = git::worktree_changes_in(root.path(), &["criv.toml", "docs/adr"]).unwrap();
        assert_eq!(
            change_violations(
                &files,
                &Config::default(),
                Some(&changes),
                ChangeMode::Commit
            )
            .len(),
            1
        );
    }

    #[test]
    /// Detect an ADR edit using the old scope when configuration moves it.
    fn scope_change_is_rejected_even_when_new_scope_hides_the_adr() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        fs::write(root.path().join("criv.toml"), "[vault]\ndocs = \"other\"\n").unwrap();
        fs::write(
            root.path().join("docs/adr/0001-base.md"),
            "Changed decision\n",
        )
        .unwrap();
        let changes = git::worktree_changes_in(root.path(), &["criv.toml", "docs/adr"]).unwrap();
        let current = Config {
            docs_dir: "other".into(),
            ..Config::default()
        };
        let violations = change_violations(&files, &current, Some(&changes), ChangeMode::Commit);
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0],
            "criv.toml moves the decision scope from `docs/adr` to `other/adr` in the same transaction as a decision change; the immutability gate would read the new scope"
        );
    }

    #[test]
    /// Distinguish a branch allocation conflict from a published ADR edit.
    fn ci_allows_branch_local_deletion_but_rejects_a_published_edit() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        git(root.path(), &["checkout", "-b", "target"]);
        fs::write(
            root.path().join("docs/adr/0002-target.md"),
            "Target decision\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "target decision"]);
        git(root.path(), &["checkout", "main"]);
        fs::write(
            root.path().join("docs/adr/0001-base.md"),
            "Changed decision\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "edit published decision"]);
        let changes = git::changes_between(root.path(), "target", "HEAD").unwrap();
        let violations =
            change_violations(&files, &Config::default(), Some(&changes), ChangeMode::Ci);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("docs/adr/0001-base.md:"));
        assert_eq!(
            change_violations(&files, &Config::default(), Some(&changes), ChangeMode::Push).len(),
            2
        );
    }

    #[test]
    /// Keep scope detection limited to decision Markdown, excluding the index.
    fn only_a_decision_file_looks_like_a_decision() {
        assert!(looks_like_decision("docs/adr/0001-example.md"));
        assert!(looks_like_decision("documentation/adr/0002-other.md"));
        assert!(!looks_like_decision("docs/adr/README.md"));
        assert!(!looks_like_decision("docs/guide.md"));
        assert!(!looks_like_decision("src/adr.rs"));
    }

    #[test]
    /// Allow new decisions while preserving the published decision contract.
    fn adr_immutability_allows_new_adrs_but_blocks_existing_changes() {
        let entries = vec![
            ChangedEntry {
                status: ChangeStatus::Added,
                path: "docs/adr/0012-new.md".into(),
                previous_path: None,
                old_ref: None,
                new_ref: None,
            },
            ChangedEntry {
                status: ChangeStatus::Modified,
                path: "docs/adr/0002-existing.md".into(),
                previous_path: None,
                old_ref: None,
                new_ref: None,
            },
            ChangedEntry {
                status: ChangeStatus::Renamed,
                path: "docs/adr/0003-renamed.md".into(),
                previous_path: Some("docs/adr/0003-existing.md".into()),
                old_ref: None,
                new_ref: None,
            },
            ChangedEntry {
                status: ChangeStatus::Modified,
                path: "docs/adr/README.md".into(),
                previous_path: None,
                old_ref: None,
                new_ref: None,
            },
        ];

        let root = tempfile::TempDir::new().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let changes = ChangedSet {
            entries,
            old_ref: None,
            new_ref: None,
            basis: "test".into(),
        };
        let violations = change_violations(
            &files,
            &Config::default(),
            Some(&changes),
            ChangeMode::Commit,
        );

        assert_eq!(violations.len(), 2);
        assert!(violations[0].contains("0002-existing"));
        assert!(violations[1].contains("0003-existing"));
    }

    #[test]
    /// Accept equivalent ADR links with matching targets and fragments.
    fn mechanical_adr_link_migrations_are_allowed() {
        let old = "See [[ADR-0010]] and [[ADR-0001#Context]].\n";
        let new = "See [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]] and [[docs/adr/0001-local-cli-vault-architecture#Context|ADR-0001#Context]].\n";

        assert!(is_mechanical_wikilink_portability_migration(old, new));
    }

    #[test]
    /// Keep a content edit from using the portable-link migration exception.
    fn mechanical_adr_link_migrations_reject_content_edits() {
        let old = "See [[ADR-0010]].\n";
        let new = "Changed decision text and see [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]].\n";

        assert!(!is_mechanical_wikilink_portability_migration(old, new));
    }

    #[test]
    /// Reject portable links whose target changes the referenced decision.
    fn mechanical_adr_link_migrations_reject_mismatched_targets() {
        let old = "See [[ADR-0010]].\n";
        let new = "See [[0011-embed-runtime-skill-templates-as-assets|ADR-0010]].\n";

        assert!(!is_mechanical_wikilink_portability_migration(old, new));
    }

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_PREFIX")
            .args(args)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn rewrites_overlapping_ids_simultaneously() {
        let mappings = vec![
            Mapping {
                old_id: 5,
                new_id: 7,
                old_path: "docs/adr/0005-a.md".into(),
                new_path: "docs/adr/0007-a.md".into(),
            },
            Mapping {
                old_id: 6,
                new_id: 8,
                old_path: "docs/adr/0006-b.md".into(),
                new_path: "docs/adr/0008-b.md".into(),
            },
        ];
        assert_eq!(
            rewrite_text(
                "ADR-0005 ADR-0006 ADR-00020 [[0005-a.md|ADR-0005]] [[docs/adr/0006-b.md#context]] [[0005-a]]",
                &mappings,
            )
            .unwrap(),
            "ADR-0007 ADR-0008 ADR-00020 [[0007-a.md|ADR-0007]] [[docs/adr/0008-b.md#context]] [[0007-a]]"
        );
    }

    #[test]
    fn keeps_incidental_text_and_placeholder_like_values_intact() {
        let mappings = vec![Mapping {
            old_id: 2,
            new_id: 3,
            old_path: "docs/adr/0002-topic.md".into(),
            new_path: "docs/adr/0003-topic.md".into(),
        }];
        assert_eq!(
            rewrite_text(
                "ADR-0002/local-id ADR-00020 XADR-0002 __CRIV_ADR_TOKEN_0__ 0002-topic.md\r\n",
                &mappings,
            )
            .unwrap(),
            "ADR-0003/local-id ADR-00020 XADR-0002 __CRIV_ADR_TOKEN_0__ 0002-topic.md\r\n"
        );
    }

    #[test]
    fn detects_supported_references_in_non_utf8_content_without_rewriting_it() {
        let mappings = vec![Mapping {
            old_id: 2,
            new_id: 3,
            old_path: "docs/adr/0002-topic.md".into(),
            new_path: "docs/adr/0003-topic.md".into(),
        }];
        assert!(contains_reference(b"\xff[[0002-topic]]", &mappings));
        assert!(contains_reference(b"\xffADR-0002/local-id", &mappings));
        assert!(!contains_reference(b"\xffADR-00020", &mappings));
    }

    #[test]
    fn retains_a_destination_that_is_another_mapping_source() {
        let mappings = vec![
            Mapping {
                old_id: 5,
                new_id: 7,
                old_path: "docs/adr/0005-a.md".into(),
                new_path: "docs/adr/0007-a.md".into(),
            },
            Mapping {
                old_id: 7,
                new_id: 8,
                old_path: "docs/adr/0007-a.md".into(),
                new_path: "docs/adr/0008-a.md".into(),
            },
        ];
        assert_eq!(superseded_paths(&mappings), vec!["docs/adr/0005-a.md"]);
    }

    #[test]
    fn detects_distinctive_content_carried_by_an_apparent_addition() {
        assert!(plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Base\nstatus: accepted\ndate: 2026-08-02\n---\n\n## Base\n\nbase\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\ndate: 2026-08-02\n---\n\n## Base\n\nbase\n",
        ));
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Topic\nstatus: accepted\ndate: 2026-08-02\n---\n\n## Topic\n\ntopic\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\ndate: 2026-08-02\n---\n\n## Base\n\nbase\n",
        ));
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Topic\nstatus: accepted\n---\n\n# Topic\n\n## Context\n\nA new topic.\n\n## Decision\n\nChoose topic.\n\n## Consequences\n\nTopic follows.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\n---\n\n# Base\n\n## Context\n\nAn existing base.\n\n## Decision\n\nChoose base.\n\n## Consequences\n\nBase follows.\n",
        ));
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Topic\nstatus: accepted\n---\n\n# Topic\n\n## Alternatives Considered\n\nA new option.\n\nRejected. Keep the current path text.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\n---\n\n# Base\n\n## Alternatives Considered\n\nAn old option.\n\nRejected. Keep the old path text.\n",
        ));
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Topic\nstatus: accepted\n---\n\n# Topic\n\nA new decision about\npath text.\n\npaths.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\n---\n\n# Base\n\nAn old decision has different\npath text.\n\npaths.\n",
        ));
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Topic\nstatus: accepted\n---\n\n# Topic\n\nShared runtime behavior remains available through the stable public command.\nShared output behavior remains available through the stable public format.\nThe new decision has enough distinct material to show independent authorship and a different purpose.\nA second new paragraph adds more distinct context, constraints, and consequences for this decision.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\n---\n\n# Base\n\nShared runtime behavior remains available through the stable public command.\nShared output behavior remains available through the stable public format.\nThe old decision has enough distinct material to show independent authorship and a different purpose.\nA second old paragraph adds more distinct context, constraints, and consequences for this decision.\n",
        ));
        assert!(plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Topic\nstatus: accepted\n---\n\n# Topic\n\nThis distinctive published requirement is long enough to identify the earlier decision and remains unchanged.\nNew material attempts to make the copied decision look independent.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Base\nstatus: accepted\n---\n\n# Base\n\nThis distinctive published requirement is long enough to identify the earlier decision and remains unchanged.\nOriginal context.\n",
        ));
        assert!(plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Shared\nstatus: accepted\n---\n\n# Shared\n\n## Context\n\nThe same substantive context is retained.\n\n## Decision\n\nA changed conclusion.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Shared\nstatus: accepted\n---\n\n# Shared\n\n## Context\n\nThe same substantive context is retained.\n\n## Decision\n\nThe original conclusion.\n",
        ));
    }

    #[test]
    fn ignores_markdown_code_fences_when_comparing_adr_content() {
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Local Viewer\nstatus: accepted\n---\n\n# Local Viewer\n\n## Decision\n\n```sh\ncriv install-editor --editor code\n```\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Offline Check\nstatus: accepted\n---\n\n# Offline Check\n\n## Decision\n\n```sh\ncriv install-editor --editor code\n```\n",
        ));
    }

    #[test]
    fn allocates_branch_adrs_contiguously_in_original_numeric_order() {
        let branch_adrs = vec![
            AdrFile {
                path: "docs/adr/0009-later.md".into(),
                id: 9,
                slug: "later".into(),
                contents: String::new(),
            },
            AdrFile {
                path: "docs/adr/0007-earlier.md".into(),
                id: 7,
                slug: "earlier".into(),
                contents: String::new(),
            },
        ];
        let mappings = allocation_mappings(&branch_adrs, 12, true).unwrap();
        assert_eq!(
            mappings,
            vec![
                Mapping {
                    old_id: 7,
                    new_id: 13,
                    old_path: "docs/adr/0007-earlier.md".into(),
                    new_path: "docs/adr/0013-earlier.md".into(),
                },
                Mapping {
                    old_id: 9,
                    new_id: 14,
                    old_path: "docs/adr/0009-later.md".into(),
                    new_path: "docs/adr/0014-later.md".into(),
                },
            ]
        );
        assert!(
            allocation_mappings(&branch_adrs, 12, false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_allocations_past_adr_9999() {
        let branch_adrs = vec![AdrFile {
            path: "docs/adr/0001-topic.md".into(),
            id: 1,
            slug: "topic".into(),
            contents: String::new(),
        }];
        let error = allocation_mappings(&branch_adrs, 9999, true).unwrap_err();
        assert!(error.to_string().contains("ADR-9999"));
    }
}

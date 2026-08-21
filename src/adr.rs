mod reconcile_transaction;
mod source_reconcile;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use clap::{Args as ClapArgs, Subcommand};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::git;
use crate::repository::RepositoryFiles;
use crate::vault::Vault;
use crate::{CrivError, Result};

use self::reconcile_transaction::Snapshot;

pub(crate) use source_reconcile::{
    allows_history_change as source_history_change_is_allowed,
    receipt_allows_transaction as source_receipt_allows_transaction,
    receipt_is_current as source_receipt_is_current,
};

const RECEIPT_SCHEMA: &str = "criv.adr-reconcile/3";
const RECEIPT_PATH: &str = ".criv/adr-reconcile.json";
const RECONCILIATION_COMMIT_MESSAGE: &str = "docs(adr): reconcile provisional identifiers";

#[derive(Debug, ClapArgs)]
pub(crate) struct AdrOptions {
    #[command(subcommand)]
    command: AdrCommand,
}

#[derive(Debug, Subcommand)]
enum AdrCommand {
    /// Reconcile provisional ADR IDs against an integration target.
    Reconcile(ReconcileOptions),
    #[command(about = "Reconcile exact governed source renames against an integration target")]
    ReconcileSources(source_reconcile::Options),
}

#[derive(Debug, ClapArgs)]
struct ReconcileOptions {
    /// Target branch or commit to compare with.
    #[arg(long)]
    base: String,
    /// Report a required reconciliation without modifying the worktree.
    #[arg(long)]
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

pub(crate) fn run(root: &Path, options: AdrOptions) -> Result<()> {
    match options.command {
        AdrCommand::Reconcile(options) => reconcile(root, options),
        AdrCommand::ReconcileSources(options) => source_reconcile::run(root, options),
    }
}

/// CI calls the same read-only planner as the user-facing `--check` command.
pub(crate) fn check_base(root: &Path, base_ref: &str) -> Result<()> {
    reconcile(
        root,
        ReconcileOptions {
            base: base_ref.to_owned(),
            check: true,
        },
    )
}

/// Local hooks accept only the complete staged transaction produced by this
/// command. Git may present its ADR move as either a rename or a deletion.
pub(crate) fn receipt_allows_change(root: &Path, entry: &git::ChangedEntry) -> bool {
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
pub(crate) fn receipt_is_current(root: &Path) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    receipt.schema == RECEIPT_SCHEMA
        && git::resolve_commit(root, "HEAD").ok().as_deref() == Some(receipt.head_sha.as_str())
}

/// Reject partial staging even when Git has no deletion entry left to send
/// through the per-ADR immutability gate (for example, after a source ADR is
/// recreated at its former path).
pub(crate) fn receipt_allows_transaction(root: &Path, entries: &[git::ChangedEntry]) -> bool {
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
pub(crate) fn receipt_allows_commit(root: &Path, commit: &str) -> bool {
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
pub(crate) fn receipt_allows_history_change(
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
                .map(|contents| hash(&contents) == source.before_hash)
                .unwrap_or(false)
                && git::file_mode(root, &receipt.head_sha, &source.path)
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(source.before_mode.as_str())
        })
}

fn receipt_tree_matches(root: &Path, receipt: &Receipt, tree: &str) -> bool {
    receipt.files.iter().all(|file| {
        let before_matches = match &file.before_hash {
            Some(before_hash) => git::blob(root, &receipt.head_sha, &file.path)
                .map(|contents| hash(&contents) == *before_hash)
                .unwrap_or(false),
            None => git::blob(root, &receipt.head_sha, &file.path).is_err(),
        };
        before_matches
            && git::file_mode(root, &receipt.head_sha, &file.path)
                .ok()
                .flatten()
                .as_deref()
                == file.before_mode.as_deref()
            && git::blob(root, tree, &file.path)
                .map(|contents| hash(&contents) == file.after_hash)
                .unwrap_or(false)
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
        git::blob(root, tree, &file.path)
            .map(|contents| hash(&contents) == file.after_hash)
            .unwrap_or(false)
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

fn reconcile(root: &Path, options: ReconcileOptions) -> Result<()> {
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
                let contents = fs::read_to_string(root.join(current_path)).map_err(|error| {
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
                let contents = fs::read_to_string(root.join(&entry.path))?;
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
        if candidate == path || !candidate.ends_with(".md") {
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
    shared_body.len() > 1 || shared_body.iter().map(|line| line.len()).sum::<usize>() >= 80
}

#[derive(Debug, Eq, PartialEq)]
struct AdrSemanticContent {
    title: Option<String>,
    body: BTreeSet<String>,
}

fn adr_semantic_content(contents: &str) -> AdrSemanticContent {
    let mut in_frontmatter = false;
    let mut title = None;
    let mut body = BTreeSet::new();
    for line in contents.lines() {
        let line = line.trim();
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
            || line.starts_with("```")
            || line.starts_with("~~~")
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
        .checked_add(ordered.len() as u32)
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
                .checked_add(offset as u32 + 1)
                .ok_or_else(|| CrivError::new("ADR ID allocation overflow"))?;
            let new_name = filename(&new_id, &adr.slug);
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
    path.starts_with(prefix) && path != format!("{prefix}README.md") && path.ends_with(".md")
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
        .is_some_and(|slugs| slugs[0] == slugs[1])
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
    let id = number.parse::<u32>().unwrap();
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

fn filename(id: &u32, slug: &str) -> String {
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
        .filter(|diagnostic| diagnostic.is_error())
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
                .map(|contents| hash(&contents) != source.before_hash)
                .unwrap_or(true)
                || git::file_mode(root, &receipt.head_sha, &source.path)
                    .ok()
                    .flatten()
                    .as_deref()
                    != Some(source.before_mode.as_str())
        })
        || receipt.files.iter().any(|file| {
            let before_matches = match &file.before_hash {
                Some(before_hash) => git::blob(root, &receipt.head_sha, &file.path)
                    .map(|contents| hash(&contents) == *before_hash)
                    .unwrap_or(false),
                None => git::blob(root, &receipt.head_sha, &file.path).is_err(),
            };
            !before_matches
                || git::file_mode(root, &receipt.head_sha, &file.path)
                    .ok()
                    .flatten()
                    .as_deref()
                    != file.before_mode.as_deref()
                || git::worktree_blob(root, &file.path)
                    .map(|contents| hash(&contents) != file.after_hash)
                    .unwrap_or(true)
                || worktree_file_mode(root, &file.path)
                    .map(|mode| mode != file.after_mode)
                    .unwrap_or(true)
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
    let mut rewrites = BTreeMap::new();
    let mut inherited_references = None;
    for path in paths {
        if path == ".criv/adr-reconcile.json" || path.starts_with(".git/") {
            continue;
        }
        let file = root.join(&path);
        if !file.is_file() {
            continue;
        }
        let contents = match fs::read_to_string(&file) {
            Ok(contents) => contents,
            Err(_) => {
                let bytes = fs::read(&file)?;
                if contains_reference(&bytes, &plan.mappings) {
                    return Err(CrivError::new(format!(
                        "refusing to reconcile binary or non-UTF-8 file `{path}` containing an ADR reference"
                    )));
                }
                continue;
            }
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
            .map(|mapping| mapping.new_path.clone())
            .unwrap_or_else(|| path.clone());
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
        let line_number = index + 1;
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
        output.replace_range(edit.start..edit.end, &edit.replacement);
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
            let end = start + old_id.len();
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
    while let Some(relative_start) = contents[cursor..].find("[[") {
        let start = cursor + relative_start;
        let body_start = start + 2;
        let Some(relative_end) = contents[body_start..].find("]]") else {
            break;
        };
        let end = body_start + relative_end;
        add_wikilink_edits(contents, body_start, end, mappings, &mut edits);
        cursor = end + 2;
    }
    edits.sort_by_key(|edit| (edit.start, edit.end));
    for pair in edits.windows(2) {
        if pair[0].end > pair[1].start
            && (pair[0].start != pair[1].start
                || pair[0].end != pair[1].end
                || pair[0].replacement != pair[1].replacement)
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
    let body = &contents[start..end];
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
                    end: part_start + link_target.len(),
                    replacement: replacement.to_owned(),
                });
            }
        }
        part_start += part.len() + 1;
    }
}

fn contains_exact_id(bytes: &[u8], id: u32) -> bool {
    let needle = format!("ADR-{id:04}");
    bytes
        .windows(needle.len())
        .enumerate()
        .any(|(start, window)| {
            window == needle.as_bytes() && is_exact_token(bytes, start, start + needle.len())
        })
}

fn contains_wikilink_reference_bytes(bytes: &[u8], mappings: &[Mapping]) -> bool {
    let mut cursor = 0;
    while let Some(relative_start) = bytes[cursor..]
        .windows(2)
        .position(|window| window == b"[[")
    {
        let start = cursor + relative_start + 2;
        let Some(relative_end) = bytes[start..].windows(2).position(|window| window == b"]]")
        else {
            break;
        };
        let body = &bytes[start..start + relative_end];
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
        cursor = start + relative_end + 2;
    }
    false
}

fn is_exact_token(bytes: &[u8], start: usize, end: usize) -> bool {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    (start == 0 || !is_word(bytes[start - 1])) && (end == bytes.len() || !is_word(bytes[end]))
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
    let receipt: Receipt = serde_json::from_str(&fs::read_to_string(
        root.join(".criv/adr-reconcile.json"),
    )?)
    .map_err(|_| {
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
        assert!(plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Shared\nstatus: accepted\n---\n\n# Shared\n\n## Context\n\nThe same substantive context is retained.\n\n## Decision\n\nA changed conclusion.\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Shared\nstatus: accepted\n---\n\n# Shared\n\n## Context\n\nThe same substantive context is retained.\n\n## Decision\n\nThe original conclusion.\n",
        ));
    }

    #[test]
    fn ignores_markdown_code_fences_when_comparing_adr_content() {
        assert!(!plausible_carried_content(
            "---\nid: ADR-0002\nkind: decision\ntitle: Local Viewer\nstatus: accepted\n---\n\n# Local Viewer\n\n## Decision\n\n```sh\ncriv install-editor --editor code\n```\n",
            "---\nid: ADR-0001\nkind: decision\ntitle: Offline Check\nstatus: accepted\n---\n\n# Offline Check\n\n## Decision\n\n```sh\nzizmor --offline --strict-collection .\n```\n",
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

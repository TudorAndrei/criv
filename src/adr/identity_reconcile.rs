//! `criv adr reconcile`: renumber branch-local ADRs that collide with the
//! integration target and rewrite every branch-owned reference to them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::receipt::{
    self, IdentityFile, IdentityMapping, IdentityReceipt, IdentitySource, Receipt,
};
use super::reconcile_transaction::{Context, FileWrite, Reconciliation, Writes};
use crate::config::{CONFIG_FILE, Config};
use crate::git::{ChangeStatus, ChangedEntry, Comparison, GitRepository, Revision};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};
use crate::{identity, markdown};

pub(super) struct IdentityReconciliation;

#[derive(Debug, Clone, Eq, PartialEq)]
struct AdrFile {
    path: String,
    id: u32,
    slug: String,
    contents: String,
}

pub(super) struct Plan {
    /// Paths and IDs currently materialized in the worktree. These are the
    /// inputs to rewriting, which can differ from the committed branch
    /// identity after an earlier receipt has been applied.
    mappings: Vec<IdentityMapping>,
    /// The equivalent transaction expressed against `head_sha`. Receipts
    /// always describe the eventual index/commit relative to that commit.
    receipt_mappings: Vec<IdentityMapping>,
    branch_adrs: Vec<AdrFile>,
    receipt_sources: Vec<AdrFile>,
    /// The worktree holds a verified receipt from the current `HEAD`.
    resumes: bool,
}

impl Reconciliation for IdentityReconciliation {
    type Plan = Plan;
    type Receipt = IdentityReceipt;
    const COMMAND: &'static str = "criv adr reconcile";
    const COMMIT_MESSAGE: &'static str = "docs(adr): reconcile provisional identifiers";
    const CURRENT: &'static str = "ADR allocation is current; no reconciliation is required";

    fn plan(context: &Context<'_>) -> Result<Plan> {
        build_plan(context)
    }

    fn is_current(plan: &Plan) -> bool {
        plan.mappings.is_empty()
    }

    fn print(plan: &Plan) {
        println!("ADR reconciliation mapping:");
        for mapping in &plan.mappings {
            println!(
                "  {} -> {} ({})",
                identity::format_adr_id(mapping.old_id),
                identity::format_adr_id(mapping.new_id),
                mapping.new_path
            );
        }
    }

    fn conflict(context: &Context<'_>, _plan: &Plan) -> String {
        format!(
            "ADR allocation conflicts with target {}",
            context.target_sha
        )
    }

    fn resumes(plan: &Plan) -> bool {
        plan.resumes
    }

    fn writes(context: &Context<'_>, plan: &Plan) -> Result<Writes> {
        let rewrites = rewrite_candidates(context, plan)?;
        // Capture every source before publishing any destination. A
        // destination may be another mapping's source.
        let mut permissions = plan
            .mappings
            .iter()
            .map(|mapping| {
                Ok((
                    mapping.new_path.as_str(),
                    context.files.permissions(Path::new(&mapping.old_path))?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Writes {
            files: rewrites
                .into_iter()
                .map(|(path, contents)| {
                    let permissions = permissions.remove(path.as_str());
                    (
                        path,
                        FileWrite {
                            contents,
                            permissions,
                        },
                    )
                })
                .collect(),
            deletions: superseded_paths(&plan.mappings)
                .into_iter()
                .chain(superseded_paths(&plan.receipt_mappings))
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        })
    }

    fn receipt(context: &Context<'_>, plan: &Plan, writes: &Writes) -> Result<IdentityReceipt> {
        let git = context.git;
        let head = Revision::Commit(context.head_sha.to_owned());
        let files = writes
            .files
            .keys()
            .map(|path| {
                let before = git.file(&head, path)?;
                let after = git.file(&Revision::Worktree, path)?.ok_or_else(|| {
                    CrivError::new(format!("reconciled file `{path}` is missing"))
                })?;
                Ok(IdentityFile {
                    path: path.clone(),
                    before_hash: before.as_ref().map(|file| receipt::hash(&file.bytes)),
                    after_hash: receipt::hash(&after.bytes),
                    before_mode: before.map(|file| file.mode),
                    after_mode: after.mode,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let sources = plan
            .receipt_sources
            .iter()
            .map(|adr| {
                let mode = git
                    .file(&head, &adr.path)?
                    .ok_or_else(|| {
                        CrivError::new(format!("cannot read Git mode for `{}`", adr.path))
                    })?
                    .mode;
                Ok(IdentitySource {
                    path: adr.path.clone(),
                    before_hash: receipt::hash(adr.contents.as_bytes()),
                    before_mode: mode,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(IdentityReceipt {
            schema: IdentityReceipt::SCHEMA.into(),
            base_ref: context.base_ref.to_owned(),
            head_sha: context.head_sha.to_owned(),
            target_sha: context.target_sha.to_owned(),
            merge_base: context.merge_base.to_owned(),
            mappings: plan.receipt_mappings.clone(),
            sources,
            deletions: superseded_paths(&plan.receipt_mappings)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            files,
        })
    }

    fn verify_written(context: &Context<'_>) -> Result<()> {
        materialized_receipt(context.files, context.git)?
            .ok_or_else(|| CrivError::new("ADR reconciliation receipt is missing"))
            .map(|_| ())
    }

    fn proves(receipt: &IdentityReceipt, git: &GitRepository, staged: &[ChangedEntry]) -> bool {
        receipt.allows_transaction(git, staged)
    }
}

/// A receipt planned from the current `HEAD` must describe the worktree
/// exactly; a receipt from an earlier `HEAD` is inert.
fn materialized_receipt(
    files: &RepositoryFiles,
    git: &GitRepository,
) -> Result<Option<IdentityReceipt>> {
    let Some(receipt) = IdentityReceipt::read(files)? else {
        return Ok(None);
    };
    if !receipt.planned_from_head(git) {
        return Ok(None);
    }
    receipt.verify_materialized(git)?;
    Ok(Some(receipt))
}

#[expect(
    clippy::too_many_lines,
    reason = "the reconciliation plan validates one complete Git and vault snapshot"
)]
fn build_plan(context: &Context<'_>) -> Result<Plan> {
    let Context {
        files,
        git,
        target_sha,
        merge_base,
        ..
    } = *context;
    let target = Revision::Commit(target_sha.to_owned());
    let config = Config::load_from(files)?;
    let target_config = git
        .file(&target, CONFIG_FILE)?
        .map(|file| {
            String::from_utf8(file.bytes)
                .map_err(|_| CrivError::new(format!("target {CONFIG_FILE} is not UTF-8")))
        })
        .transpose()?;
    let target_config = Config::parse(target_config.as_deref())?;
    if config.docs_dir != target_config.docs_dir || config.adr_dir != target_config.adr_dir {
        return Err(CrivError::new(
            "refusing ADR reconciliation because vault.docs or vault.adr differs from the target; cannot prove ADR ownership",
        ));
    }
    let adr_prefix = config.adr_prefix();
    let target_paths = git.tree_paths(target_sha, &adr_prefix)?;
    let merge_paths = git
        .tree_paths(merge_base, &adr_prefix)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let target_adrs = target_paths
        .iter()
        .filter(|path| config.is_adr_path(path))
        .map(|path| parse_adr(path, git.text(&target, path)?))
        .collect::<Result<Vec<_>>>()?;
    ensure_unique(&target_adrs, "target")?;
    let target_by_id = target_adrs
        .iter()
        .map(|adr| (adr.id, adr))
        .collect::<BTreeMap<_, _>>();
    let target_paths = target_paths.into_iter().collect::<BTreeSet<_>>();
    let provenance = Provenance {
        git,
        merge_base,
        target_sha,
        merge_paths: &merge_paths,
        target_paths: &target_paths,
    };

    let materialized = materialized_receipt(files, git)?;
    let prior_mappings = materialized
        .as_ref()
        .map(|receipt| receipt.mappings.as_slice())
        .unwrap_or_default();
    let current_paths = prior_mappings
        .iter()
        .map(|mapping| (mapping.old_path.as_str(), mapping.new_path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let history_changes = git.changed_set(
        &Comparison::Trees {
            old_ref: merge_base,
            new_ref: "HEAD",
        },
        &[],
    )?;
    let changes = git.changed_set(
        &Comparison::Trees {
            old_ref: merge_base,
            new_ref: "HEAD",
        },
        &[&adr_prefix],
    )?;
    let worktree_changes =
        git.changed_set(&Comparison::Worktree { old_ref: "HEAD" }, &[&adr_prefix])?;
    let worktree_moves = worktree_changes
        .entries
        .iter()
        .filter(|entry| matches!(entry.status, ChangeStatus::Renamed | ChangeStatus::Copied))
        .filter_map(|entry| {
            entry
                .previous_path
                .as_deref()
                .map(|previous| (previous, entry.path.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let worktree_additions = worktree_changes
        .entries
        .iter()
        .filter(|entry| entry.status == ChangeStatus::Added)
        .filter(|entry| config.is_adr_path(&entry.path))
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    let mut receipt_sources = Vec::new();
    let mut branch_adrs = changes
        .entries
        .iter()
        .filter(|entry| config.is_adr_path(&entry.path))
        .filter(|entry| !receipt::source_history_allows(files, git, &history_changes, entry))
        .map(|entry| match entry.status {
            ChangeStatus::Added => {
                let current_path = current_paths
                    .get(entry.path.as_str())
                    .or_else(|| worktree_moves.get(entry.path.as_str()))
                    .copied()
                    .or_else(|| {
                        let mut candidates = worktree_additions
                            .iter()
                            .filter(|candidate| same_adr_slug(&entry.path, candidate));
                        let candidate = *candidates.next()?;
                        candidates.next().is_none().then_some(candidate)
                    })
                    .unwrap_or(&entry.path);
                let contents = files.read_string(Path::new(current_path)).map_err(|error| {
                    CrivError::new(format!(
                        "cannot read branch-created ADR `{current_path}` while proving ownership: {error}"
                    ))
                })?;
                provenance.ensure_new(&entry.path, &contents)?;
                let adr = parse_adr(current_path, contents)?;
                receipt_sources.push(parse_adr(
                    &entry.path,
                    git.text(&Revision::Commit("HEAD".into()), &entry.path)?,
                )?);
                Ok(adr)
            }
            ChangeStatus::Renamed | ChangeStatus::Copied => Err(CrivError::new(format!(
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
        if !config.is_adr_path(&entry.path) {
            continue;
        }
        match entry.status {
            ChangeStatus::Added if branch_paths.insert(entry.path.clone()) => {
                let contents = files.read_string(Path::new(&entry.path))?;
                provenance.ensure_new(&entry.path, &contents)?;
                branch_adrs.push(parse_adr(&entry.path, contents)?);
            }
            ChangeStatus::Renamed | ChangeStatus::Copied
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

    let target_max = target_adrs.iter().map(|adr| adr.id).max().unwrap_or(0);
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
                .iter()
                .find(|prior| prior.new_path == mapping.old_path)
                .unwrap_or(mapping);
            IdentityMapping {
                old_id: original.old_id,
                new_id: mapping.new_id,
                old_path: original.old_path.clone(),
                new_path: mapping.new_path.clone(),
            }
        })
        .collect::<Vec<_>>();
    let destination_paths = mappings
        .iter()
        .map(|mapping| mapping.new_path.as_str())
        .collect::<BTreeSet<_>>();
    if destination_paths.len() != mappings.len() {
        return Err(CrivError::new("ADR reconciliation destinations collide"));
    }
    for mapping in &mappings {
        if mapping.old_path != mapping.new_path
            && files.entry_exists(Path::new(&mapping.new_path))?
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
    Ok(Plan {
        mappings,
        receipt_mappings,
        branch_adrs,
        receipt_sources,
        resumes: materialized.is_some(),
    })
}

/// The published ADRs a branch-created ADR must not carry content from.
struct Provenance<'a> {
    git: &'a GitRepository,
    merge_base: &'a str,
    target_sha: &'a str,
    merge_paths: &'a BTreeSet<String>,
    target_paths: &'a BTreeSet<String>,
}

impl Provenance<'_> {
    /// Git's `A` status is not proof of authorship: an inherited ADR can be
    /// copied and changed enough to miss its normal rename threshold. Reject
    /// a new-looking file when it retains distinctive published material.
    fn ensure_new(&self, path: &str, contents: &str) -> Result<()> {
        let candidates = self
            .merge_paths
            .iter()
            .map(|path| (self.merge_base, path))
            .chain(self.target_paths.iter().map(|path| (self.target_sha, path)));
        for (revision, candidate) in candidates {
            if candidate == path
                || !Path::new(candidate)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                continue;
            }
            let source = self
                .git
                .text(&Revision::Commit(revision.to_owned()), candidate)?;
            if plausible_carried_content(contents, &source) {
                return Err(CrivError::new(format!(
                    "ADR `{path}` appears to carry published content from `{candidate}`; copied or renamed ADRs are immutable"
                )));
            }
        }
        Ok(())
    }
}

/// Compare ADR meaning rather than its standard template. Identity and
/// governance metadata, the rendered title, and conventional section headings
/// are scaffolding shared by independent decisions and cannot prove copying.
fn plausible_carried_content(first: &str, second: &str) -> bool {
    if first == second {
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
    let body_chars = |body: &BTreeSet<String>| body.iter().map(String::len).sum::<usize>();
    let smaller_body_chars = body_chars(&first.body).min(body_chars(&second.body));
    shared_chars >= 80 && shared_chars.saturating_mul(2) >= smaller_body_chars
}

#[derive(Debug, Eq, PartialEq)]
struct AdrSemanticContent {
    title: Option<String>,
    body: BTreeSet<String>,
}

fn adr_semantic_content(contents: &str) -> AdrSemanticContent {
    let frontmatter = markdown::split_frontmatter(contents);
    let title = frontmatter.and_then(|frontmatter| frontmatter.scalar("title"));
    let body_start = frontmatter.map_or(0, |frontmatter| frontmatter.body_start);
    let mut in_code_fence = false;
    let mut body = BTreeSet::new();
    for line in contents.get(body_start..).unwrap_or_default().lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
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
) -> Result<Vec<IdentityMapping>> {
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
    if allocation_end > identity::MAX_ADR_NUMBER {
        return Err(CrivError::new(format!(
            "ADR reconciliation allocation exceeds {}",
            identity::format_adr_id(identity::MAX_ADR_NUMBER)
        )));
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
            let new_name = identity::adr_filename(new_id, &adr.slug);
            let parent = Path::new(&adr.path)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            Ok(IdentityMapping {
                old_id: adr.id,
                new_id,
                old_path: adr.path.clone(),
                new_path: parent.join(new_name).to_string_lossy().replace('\\', "/"),
            })
        })
        .collect()
}

fn adr_stem(path: &str) -> Option<(u32, Option<&str>)> {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(identity::parse_adr_stem)
}

fn same_adr_slug(first: &str, second: &str) -> bool {
    adr_stem(first)
        .and_then(|(_, slug)| slug)
        .zip(adr_stem(second).and_then(|(_, slug)| slug))
        .is_some_and(|(first, second)| first == second)
}

fn parse_adr(path: &str, contents: String) -> Result<AdrFile> {
    if Path::new(path)
        .extension()
        .is_none_or(|extension| extension != "md")
    {
        return Err(CrivError::new(format!("ADR `{path}` must be Markdown")));
    }
    let Some((id, Some(slug))) = adr_stem(path).filter(|(_, slug)| slug != &Some("")) else {
        return Err(CrivError::new(format!(
            "branch-local ADR `{path}` must use NNNN-kebab-title.md"
        )));
    };
    let frontmatter = markdown::split_frontmatter(&contents).ok_or_else(|| {
        CrivError::new(format!("branch-local ADR `{path}` has no YAML frontmatter"))
    })?;
    if frontmatter
        .scalar("id")
        .as_deref()
        .and_then(identity::parse_adr_id)
        != Some(id)
    {
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
                "duplicate {owner} ADR ID {}",
                identity::format_adr_id(adr.id)
            )));
        }
    }
    Ok(())
}

/// A destination can also be another mapping's source. In that case its old
/// contents have already been copied to the later destination, and removing
/// the source would erase the newly published earlier ADR.
fn superseded_paths(mappings: &[IdentityMapping]) -> Vec<&str> {
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
fn rewrite_candidates(context: &Context<'_>, plan: &Plan) -> Result<BTreeMap<String, String>> {
    let Context {
        files,
        git,
        merge_base,
        ..
    } = *context;
    let changed_paths = git
        .changed_set(
            &Comparison::Trees {
                old_ref: merge_base,
                new_ref: "HEAD",
            },
            &[],
        )?
        .entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut paths = git.tree_paths("HEAD", ".")?;
    for adr in &plan.branch_adrs {
        if !paths.contains(&adr.path) {
            paths.push(adr.path.clone());
        }
    }
    let ownership = Rewriter {
        git,
        merge_base,
        mappings: &plan.mappings,
    };
    let mut rewrites = BTreeMap::new();
    let mut inherited_references = None;
    for path in paths {
        if path == IdentityReceipt::PATH || path.starts_with(".git/") {
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
                Some(entry) if entry.status == ChangeStatus::Added => {
                    if reference_edits(&contents, &plan.mappings)?.is_empty() {
                        contents.clone()
                    } else {
                        let evidence = match &inherited_references {
                            Some(evidence) => evidence,
                            None => inherited_references.insert(ownership.inherited_references()?),
                        };
                        match inherited_reference_source(
                            &path,
                            &contents,
                            evidence,
                            &plan.mappings,
                        )? {
                            Some(source) => ownership.owned_lines(&contents, &source, &path)?,
                            None => rewrite_text(&contents, &plan.mappings)?,
                        }
                    }
                }
                Some(entry)
                    if matches!(entry.status, ChangeStatus::Renamed | ChangeStatus::Copied) =>
                {
                    let original = entry.previous_path.as_deref().ok_or_else(|| {
                        CrivError::new(format!("Git did not report an inherited path for `{path}`"))
                    })?;
                    ownership.owned_lines(&contents, original, &path)?
                }
                _ => ownership.owned_lines(&contents, &path, &path)?,
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

/// Rewrites only the reference lines the branch added since the merge base.
struct Rewriter<'a> {
    git: &'a GitRepository,
    merge_base: &'a str,
    mappings: &'a [IdentityMapping],
}

impl Rewriter<'_> {
    /// Index exact reference-bearing lines from the merge base once. Only
    /// those lines are relevant to rewrite ownership.
    fn inherited_references(&self) -> Result<BTreeMap<String, BTreeSet<String>>> {
        let merge_base = Revision::Commit(self.merge_base.to_owned());
        let mut evidence = BTreeMap::<String, BTreeSet<String>>::new();
        for candidate in self.git.tree_paths(self.merge_base, ".")? {
            let Some(file) = self.git.file(&merge_base, &candidate)? else {
                continue;
            };
            let Ok(contents) = String::from_utf8(file.bytes) else {
                continue;
            };
            for line in contents.split_inclusive('\n') {
                if contains_reference(line.as_bytes(), self.mappings) {
                    evidence
                        .entry(git_comparison_line(line))
                        .or_default()
                        .insert(candidate.clone());
                }
            }
        }
        Ok(evidence)
    }

    fn owned_lines(&self, contents: &str, original_path: &str, path: &str) -> Result<String> {
        let ranges = self
            .git
            .added_lines(self.merge_base, original_path, "HEAD", path)?;
        let mut output = String::new();
        for (index, line) in contents.split_inclusive('\n').enumerate() {
            let line_number = index.saturating_add(1);
            if ranges.iter().any(|range| range.contains(&line_number)) {
                output.push_str(&rewrite_text(line, self.mappings)?);
            } else if contains_reference(line.as_bytes(), self.mappings) {
                return Err(CrivError::new(format!(
                    "refusing to rewrite target-owned reference in `{path}` line {line_number}"
                )));
            } else {
                output.push_str(line);
            }
        }
        Ok(output)
    }
}

fn inherited_reference_source(
    path: &str,
    contents: &str,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    mappings: &[IdentityMapping],
) -> Result<Option<String>> {
    let sources = contents
        .split_inclusive('\n')
        .filter(|line| contains_reference(line.as_bytes(), mappings))
        .filter_map(|line| evidence.get(&git_comparison_line(line)))
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

fn git_comparison_line(line: &str) -> String {
    line.strip_suffix("\r\n")
        .map_or_else(|| line.to_owned(), |line| format!("{line}\n"))
}

/// Non-UTF-8 bytes read as replacement characters, which neither form nor
/// extend an ADR id or a wiki-link. An ambiguous overlap counts as a
/// reference.
fn contains_reference(bytes: &[u8], mappings: &[IdentityMapping]) -> bool {
    reference_edits(&String::from_utf8_lossy(bytes), mappings)
        .map_or(true, |edits| !edits.is_empty())
}

#[derive(Debug, Eq, PartialEq)]
struct TextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

fn rewrite_text(contents: &str, mappings: &[IdentityMapping]) -> Result<String> {
    let mut output = contents.to_owned();
    for edit in reference_edits(contents, mappings)?.into_iter().rev() {
        if output.get(edit.start..edit.end).is_none() {
            return Err(CrivError::new(format!(
                "ADR reference edit {}..{} is not a text range",
                edit.start, edit.end
            )));
        }
        output.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok(output)
}

fn reference_edits(contents: &str, mappings: &[IdentityMapping]) -> Result<Vec<TextEdit>> {
    let bytes = contents.as_bytes();
    let mut edits = Vec::new();
    for mapping in mappings {
        let old_id = identity::format_adr_id(mapping.old_id);
        let new_id = identity::format_adr_id(mapping.new_id);
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
    for span in markdown::wiki_link_spans(contents) {
        add_wikilink_edits(contents, span.body, mappings, &mut edits);
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

/// Rewrite the link target and the alias when either names a mapped ADR file.
fn add_wikilink_edits(
    contents: &str,
    body: std::ops::Range<usize>,
    mappings: &[IdentityMapping],
    edits: &mut Vec<TextEdit>,
) {
    let Some(text) = contents.get(body.clone()) else {
        return;
    };
    let (target, alias) = markdown::split_alias(text);
    let alias_start = body.start.saturating_add(target.len()).saturating_add(1);
    for (part_start, part) in
        std::iter::once((body.start, target)).chain(alias.map(|alias| (alias_start, alias)))
    {
        let link_target = markdown::split_fragment(part).0;
        for mapping in mappings {
            let old_name = file_name(&mapping.old_path);
            let new_name = file_name(&mapping.new_path);
            let replacement = if link_target == mapping.old_path {
                Some(mapping.new_path.as_str())
            } else if link_target == old_name {
                Some(new_name)
            } else if link_target == old_name.strip_suffix(".md").unwrap_or(old_name) {
                Some(new_name.strip_suffix(".md").unwrap_or(new_name))
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
    }
}

fn file_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(old_id: u32, new_id: u32, old_path: &str, new_path: &str) -> IdentityMapping {
        IdentityMapping {
            old_id,
            new_id,
            old_path: old_path.into(),
            new_path: new_path.into(),
        }
    }

    #[test]
    fn rewrites_overlapping_ids_simultaneously() {
        let mappings = vec![
            mapping(5, 7, "docs/adr/0005-a.md", "docs/adr/0007-a.md"),
            mapping(6, 8, "docs/adr/0006-b.md", "docs/adr/0008-b.md"),
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
        let mappings = vec![mapping(
            2,
            3,
            "docs/adr/0002-topic.md",
            "docs/adr/0003-topic.md",
        )];
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
        let mappings = vec![mapping(
            2,
            3,
            "docs/adr/0002-topic.md",
            "docs/adr/0003-topic.md",
        )];
        assert!(contains_reference(b"\xff[[0002-topic]]", &mappings));
        assert!(contains_reference(b"\xffADR-0002/local-id", &mappings));
        assert!(!contains_reference(b"\xffADR-00020", &mappings));
    }

    #[test]
    fn retains_a_destination_that_is_another_mapping_source() {
        let mappings = vec![
            mapping(5, 7, "docs/adr/0005-a.md", "docs/adr/0007-a.md"),
            mapping(7, 8, "docs/adr/0007-a.md", "docs/adr/0008-a.md"),
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
    fn a_quoted_frontmatter_id_agrees_with_its_filename() {
        let adr = parse_adr(
            "docs/adr/0004-quoted.md",
            "---\nid: \"ADR-0004\"\nkind: decision\ntitle: Quoted\nstatus: draft\n---\n".into(),
        )
        .unwrap();

        assert_eq!(adr.id, 4);
    }

    #[test]
    fn allocates_branch_adrs_contiguously_in_original_numeric_order() {
        let adr = |path: &str, id, slug: &str| AdrFile {
            path: path.into(),
            id,
            slug: slug.into(),
            contents: String::new(),
        };
        let branch_adrs = vec![
            adr("docs/adr/0009-later.md", 9, "later"),
            adr("docs/adr/0007-earlier.md", 7, "earlier"),
        ];
        assert_eq!(
            allocation_mappings(&branch_adrs, 12, true).unwrap(),
            vec![
                mapping(
                    7,
                    13,
                    "docs/adr/0007-earlier.md",
                    "docs/adr/0013-earlier.md"
                ),
                mapping(9, 14, "docs/adr/0009-later.md", "docs/adr/0014-later.md"),
            ]
        );
        assert!(
            allocation_mappings(&branch_adrs, 12, false)
                .unwrap()
                .is_empty()
        );
        let error = allocation_mappings(&[adr("docs/adr/0001-topic.md", 1, "topic")], 9999, true)
            .unwrap_err();
        assert!(error.to_string().contains("ADR-9999"));
    }
}

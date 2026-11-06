//! `criv adr reconcile-sources`: follow exact Git renames in the `governs:`
//! paths of accepted decisions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::governs;
use super::receipt::{self, Receipt, SourceFile, SourceMapping, SourceReceipt};
use super::reconcile_transaction::{Context, FileWrite, Reconciliation, Writes};
use crate::git::{ChangeStatus, ChangedEntry, Comparison, GitRepository};
use crate::vault::{LoadOptions, Vault};
use crate::{CrivError, Result};

pub(super) struct SourceReconciliation;

pub(super) struct Plan {
    mappings: Vec<SourceMapping>,
    files: Vec<PlannedFile>,
}

struct PlannedFile {
    path: String,
    before: String,
    after: String,
}

impl Reconciliation for SourceReconciliation {
    type Plan = Plan;
    type Receipt = SourceReceipt;
    const COMMAND: &'static str = "criv adr reconcile-sources";
    const COMMIT_MESSAGE: &'static str = "docs(adr): reconcile renamed source scopes";
    const CURRENT: &'static str =
        "governed source paths are current; no reconciliation is required";

    fn plan(context: &Context<'_>) -> Result<Plan> {
        let vault = Vault::load(context.files, LoadOptions::default())?;
        let changes = context.git.changed_set(
            &Comparison::Trees {
                old_ref: context.merge_base,
                new_ref: context.head_sha,
            },
            &[],
        )?;
        let required = required_reconciliations(&vault, &changes.entries)?;

        let mut files = Vec::new();
        if !required.is_empty() {
            for note in vault
                .notes()
                .iter()
                .filter(|note| vault.is_effective_decision(note))
            {
                if !note
                    .governs()
                    .iter()
                    .any(|path| required.contains_key(path))
                {
                    continue;
                }
                let before = context.files.read_string(Path::new(note.rel_path()))?;
                let after = governs::rewrite(&before, &required)?;
                if before != after {
                    files.push(PlannedFile {
                        path: note.rel_path().to_string(),
                        before,
                        after,
                    });
                }
            }
        }
        let mut used = BTreeSet::new();
        for file in &files {
            used.extend(governs::replaced_values(
                &file.before,
                &file.after,
                &required,
            )?);
        }
        let expected = required.keys().cloned().collect::<BTreeSet<_>>();
        if used != expected {
            return Err(CrivError::new(format!(
                "unsupported `governs:` YAML prevented exact source reconciliation: {}",
                expected
                    .difference(&used)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        Ok(Plan {
            mappings: required
                .into_iter()
                .map(|(old_path, new_path)| SourceMapping { old_path, new_path })
                .collect(),
            files,
        })
    }

    fn is_current(plan: &Plan) -> bool {
        plan.files.is_empty()
    }

    fn print(plan: &Plan) {
        println!("source reconciliation mapping:");
        for mapping in &plan.mappings {
            println!("  {} -> {}", mapping.old_path, mapping.new_path);
        }
        println!(
            "ADR files: {}",
            plan.files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    fn conflict(_context: &Context<'_>, _plan: &Plan) -> String {
        "governed source paths require reconciliation".into()
    }

    fn resumes(_plan: &Plan) -> bool {
        false
    }

    fn writes(_context: &Context<'_>, plan: &Plan) -> Result<Writes> {
        Ok(Writes {
            files: plan
                .files
                .iter()
                .map(|file| {
                    (
                        file.path.clone(),
                        FileWrite {
                            contents: file.after.clone(),
                            permissions: None,
                        },
                    )
                })
                .collect(),
            deletions: Vec::new(),
        })
    }

    fn receipt(context: &Context<'_>, plan: &Plan, _writes: &Writes) -> Result<SourceReceipt> {
        Ok(SourceReceipt {
            schema: SourceReceipt::SCHEMA.into(),
            base_ref: context.base_ref.to_owned(),
            target_sha: context.target_sha.to_owned(),
            head_sha: context.head_sha.to_owned(),
            merge_base: context.merge_base.to_owned(),
            mappings: plan.mappings.clone(),
            files: plan
                .files
                .iter()
                .map(|file| SourceFile {
                    path: file.path.clone(),
                    before_hash: receipt::hash(file.before.as_bytes()),
                    after_hash: receipt::hash(file.after.as_bytes()),
                })
                .collect(),
        })
    }

    fn proves(receipt: &SourceReceipt, git: &GitRepository, staged: &[ChangedEntry]) -> bool {
        receipt.allows_transaction(git, staged)
    }
}

fn required_reconciliations(
    vault: &Vault,
    entries: &[ChangedEntry],
) -> Result<BTreeMap<String, String>> {
    let mappings = receipt::rename_mappings(entries)?;
    let mapping_by_old = mappings
        .iter()
        .map(|mapping| (mapping.old_path.as_str(), mapping.new_path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let with_status = |status| entries.iter().filter(move |entry| entry.status == status);
    let deleted = with_status(ChangeStatus::Deleted)
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let copied = with_status(ChangeStatus::Copied)
        .filter_map(|entry| entry.previous_path.as_deref())
        .collect::<BTreeSet<_>>();
    let current_sources = vault
        .source_files()
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut required = BTreeMap::new();
    for note in vault
        .notes()
        .iter()
        .filter(|note| vault.is_effective_decision(note))
    {
        let matches = vault.source_globs_have_matches(note.governs());
        for (governs, has_match) in note.governs().iter().zip(matches) {
            if has_match {
                continue;
            }
            if let Some(new_path) = mapping_by_old.get(governs.as_str()) {
                if !current_sources.contains(new_path) {
                    return Err(CrivError::new(format!(
                        "cannot reconcile `{governs}` to `{new_path}` because the destination is not in the current source catalog"
                    )));
                }
                required.insert(governs.clone(), (*new_path).to_string());
                continue;
            }
            if deleted.contains(governs.as_str()) {
                return Err(successor_required(governs, "was deleted"));
            }
            if copied.contains(governs.as_str()) {
                return Err(successor_required(
                    governs,
                    "was copied rather than renamed",
                ));
            }
            return Err(successor_required(
                governs,
                "is unresolved and has no one-to-one Git rename",
            ));
        }
    }
    Ok(required)
}

fn successor_required(path: &str, reason: &str) -> CrivError {
    CrivError::new(format!(
        "cannot reconcile governed source `{path}` because it {reason}; add a new accepted ADR with `supersedes:` and the surviving scope"
    ))
}

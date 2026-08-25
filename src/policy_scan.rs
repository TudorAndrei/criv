use std::collections::BTreeSet;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use crate::Result;
use crate::diagnostic::SourceLocation;
use crate::structural::{self, CompiledPolicy, PolicyCompileError, PolicyScanRequest};
use crate::vault::{NoteKind, Vault};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PolicyDiagnosticKind {
    MissingId,
    EmptyId,
    DuplicateId { id: String },
    MissingDefinition { id: String },
    MissingLanguage { id: String },
    AmbiguousBody { id: String },
    MissingBody { id: String },
    InvalidPattern { id: String, error: String },
    InvalidRule { id: String, error: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyDiagnostic {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) kind: PolicyDiagnosticKind,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyViolation {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) adr_id: String,
    pub(crate) pattern_id: String,
    pub(crate) text: String,
    pub(crate) location: Option<SourceLocation>,
}

pub struct PolicyScanPlan {
    diagnostics: Vec<PolicyDiagnostic>,
    owners: Vec<PlannedOwner>,
    state_definition_error: Option<String>,
}

pub struct PlannedOwner {
    adr_id: String,
    scopes: Vec<String>,
    paths: BTreeSet<String>,
    policies: Vec<PlannedPolicy>,
}

pub struct PlannedPolicy {
    pattern_id: String,
    state: Option<PlannedStatePolicy>,
    compiled: CompiledPolicy,
}

struct CandidatePolicy {
    pattern_id: String,
    state_pattern_id: Option<String>,
    fingerprint_material: String,
    compiled: CompiledPolicy,
}

struct PlannedStatePolicy {
    pattern_id: String,
    input_fingerprint: String,
}

enum ScanPaths<'a> {
    Borrowed(&'a BTreeSet<String>),
    Filtered(BTreeSet<String>),
}

impl ScanPaths<'_> {
    const fn as_set(&self) -> &BTreeSet<String> {
        match self {
            Self::Borrowed(paths) => paths,
            Self::Filtered(paths) => paths,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) definition_compilations: usize,
    pub(crate) adr_scope_resolutions: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        definition_compilations: 0,
        adr_scope_resolutions: 0,
    }) };
}

#[cfg(test)]
fn record_work(record: impl FnOnce(&mut WorkCounts)) {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        record(&mut next);
        counts.set(next);
    });
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

impl PolicyScanPlan {
    pub(crate) fn new(vault: &Vault) -> Self {
        let mut diagnostics = Vec::new();
        let mut owners = Vec::new();
        let mut state_definition_errors = Vec::new();

        for note in &vault.notes {
            let reports_diagnostics = note.kind == NoteKind::Decision;
            let active_adr_id = vault
                .is_effective_decision(note)
                .then_some(note.id.as_deref())
                .flatten();
            if !reports_diagnostics && active_adr_id.is_none() {
                continue;
            }

            let mut ids = BTreeSet::new();
            let mut candidates = Vec::new();
            for policy in &note.policy_patterns {
                let Some(raw_local_id) = policy.id.as_deref() else {
                    if reports_diagnostics {
                        diagnostics.push(PolicyDiagnostic {
                            path: note.rel_path.clone(),
                            line: policy.line,
                            kind: PolicyDiagnosticKind::MissingId,
                        });
                    }
                    continue;
                };

                let local_id = raw_local_id.trim();
                if local_id.is_empty() {
                    if reports_diagnostics {
                        diagnostics.push(PolicyDiagnostic {
                            path: note.rel_path.clone(),
                            line: policy.line,
                            kind: PolicyDiagnosticKind::EmptyId,
                        });
                    }
                    continue;
                }

                if reports_diagnostics && !ids.insert(local_id.to_string()) {
                    diagnostics.push(PolicyDiagnostic {
                        path: note.rel_path.clone(),
                        line: policy.line,
                        kind: PolicyDiagnosticKind::DuplicateId {
                            id: local_id.to_string(),
                        },
                    });
                }

                #[cfg(test)]
                record_work(|counts| {
                    counts.definition_compilations =
                        counts.definition_compilations.saturating_add(1);
                });
                let state_pattern_id = active_adr_id.and_then(|adr_id| {
                    let pattern_id = format!("{adr_id}/{raw_local_id}");
                    (vault.patterns().contains(&pattern_id)
                        && vault.resolve_policy_pattern(&pattern_id).is_some_and(
                            |(registered_note, registered_policy)| {
                                std::ptr::eq(registered_note, note)
                                    && std::ptr::eq(registered_policy, policy)
                            },
                        ))
                    .then_some(pattern_id)
                });
                match structural::compile_policy(policy) {
                    Ok(compiled) => {
                        if let Some(adr_id) = active_adr_id {
                            candidates.push(CandidatePolicy {
                                pattern_id: format!("{adr_id}/{local_id}"),
                                state_pattern_id,
                                fingerprint_material: format!("{policy:#?}"),
                                compiled,
                            });
                        }
                    }
                    Err(error) => {
                        if let Some(pattern_id) = state_pattern_id {
                            state_definition_errors.push((pattern_id, error.to_string()));
                        }
                        if reports_diagnostics {
                            diagnostics.push(PolicyDiagnostic {
                                path: note.rel_path.clone(),
                                line: policy.line,
                                kind: diagnostic_kind(local_id, error),
                            });
                        }
                    }
                }
            }

            let Some(adr_id) = active_adr_id else {
                continue;
            };
            if candidates.is_empty() {
                continue;
            }

            #[cfg(test)]
            record_work(|counts| {
                counts.adr_scope_resolutions = counts.adr_scope_resolutions.saturating_add(1);
            });
            let scopes = Vault::effective_governs(note);
            let paths = vault
                .source_files_matching_globs(&scopes)
                .into_iter()
                .collect();
            let policies = candidates
                .into_iter()
                .map(|candidate| PlannedPolicy {
                    pattern_id: candidate.pattern_id,
                    state: candidate
                        .state_pattern_id
                        .map(|pattern_id| PlannedStatePolicy {
                            pattern_id,
                            input_fingerprint: blake3::hash(
                                format!("{}\0{scopes:?}", candidate.fingerprint_material)
                                    .as_bytes(),
                            )
                            .to_hex()
                            .to_string(),
                        }),
                    compiled: candidate.compiled,
                })
                .collect();
            owners.push(PlannedOwner {
                adr_id: adr_id.to_string(),
                scopes,
                paths,
                policies,
            });
        }

        state_definition_errors.sort_by(|left, right| left.0.cmp(&right.0));
        Self {
            diagnostics,
            owners,
            state_definition_error: state_definition_errors
                .into_iter()
                .next()
                .map(|(_, error)| error),
        }
    }

    pub(crate) fn definition_diagnostics(&self) -> &[PolicyDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn owners(&self) -> &[PlannedOwner] {
        &self.owners
    }

    pub(crate) fn state_definition_error(&self) -> Option<&str> {
        self.state_definition_error.as_deref()
    }

    pub(crate) fn scan(
        &self,
        vault: &Vault,
        changed_files: Option<&BTreeSet<String>>,
    ) -> Result<Vec<PolicyViolation>> {
        let scan_paths = self
            .owners
            .iter()
            .map(|owner| {
                changed_files.map_or(ScanPaths::Borrowed(&owner.paths), |changed| {
                    ScanPaths::Filtered(
                        owner
                            .paths
                            .iter()
                            .filter(|path| changed.contains(*path))
                            .cloned()
                            .collect(),
                    )
                })
            })
            .collect::<Vec<_>>();

        let mut records = Vec::new();
        let mut requests = Vec::new();
        for (owner_index, owner) in self.owners.iter().enumerate() {
            for policy in &owner.policies {
                let key = records.len();
                records.push((owner, policy));
                requests.push(PolicyScanRequest {
                    key,
                    policy: &policy.compiled,
                    paths: scan_paths
                        .get(owner_index)
                        .map_or(&owner.paths, ScanPaths::as_set),
                });
            }
        }

        let rows_by_key = structural::find_policies_batch(vault, &requests)?;
        let mut violations = Vec::new();
        for (key, (owner, policy)) in records.into_iter().enumerate() {
            if let Some(rows) = rows_by_key.get(&key) {
                violations.extend(rows.iter().map(|row| PolicyViolation {
                    path: row.path.clone(),
                    line: row.line,
                    adr_id: owner.adr_id.clone(),
                    pattern_id: policy.pattern_id.clone(),
                    text: row.text.clone(),
                    location: row.location.clone(),
                }));
            }
        }
        Ok(violations)
    }
}

impl PlannedOwner {
    pub(crate) fn scopes(&self) -> &[String] {
        &self.scopes
    }

    pub(crate) const fn paths(&self) -> &BTreeSet<String> {
        &self.paths
    }

    pub(crate) fn policies(&self) -> &[PlannedPolicy] {
        &self.policies
    }
}

impl PlannedPolicy {
    pub(crate) fn state_pattern_id(&self) -> Option<&str> {
        self.state.as_ref().map(|state| state.pattern_id.as_str())
    }

    pub(crate) fn state_input_fingerprint(&self) -> Option<&str> {
        self.state
            .as_ref()
            .map(|state| state.input_fingerprint.as_str())
    }

    pub(crate) const fn compiled(&self) -> &CompiledPolicy {
        &self.compiled
    }
}

fn diagnostic_kind(local_id: &str, error: PolicyCompileError) -> PolicyDiagnosticKind {
    let id = local_id.to_string();
    match error {
        PolicyCompileError::MissingDefinition => PolicyDiagnosticKind::MissingDefinition { id },
        PolicyCompileError::MissingLanguage => PolicyDiagnosticKind::MissingLanguage { id },
        PolicyCompileError::AmbiguousBody => PolicyDiagnosticKind::AmbiguousBody { id },
        PolicyCompileError::MissingBody => PolicyDiagnosticKind::MissingBody { id },
        PolicyCompileError::InvalidPattern(error) => {
            PolicyDiagnosticKind::InvalidPattern { id, error }
        }
        PolicyCompileError::InvalidRule(error) => PolicyDiagnosticKind::InvalidRule { id, error },
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;
    use crate::identity::copy_fixture_tree;

    #[test]
    fn policy_free_accepted_adrs_resolve_no_policy_scopes() {
        let (_temp, vault) = policy_fixture("policy-free");
        reset_work_counts();
        structural::reset_work_counts();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert!(plan.diagnostics.is_empty());
        assert_eq!(work_counts(), WorkCounts::default());
        assert_eq!(structural::work_counts().policy_compilations, 0);
    }

    #[test]
    fn owner_scope_and_compiled_policies_are_reused_by_the_batch() {
        let (_temp, vault) = policy_fixture("two-policies");
        reset_work_counts();
        structural::reset_work_counts();

        let plan = PolicyScanPlan::new(&vault);

        assert_eq!(
            work_counts(),
            WorkCounts {
                definition_compilations: 2,
                adr_scope_resolutions: 1,
            }
        );
        assert_eq!(structural::work_counts().policy_compilations, 2);
        assert_eq!(plan.owners.len(), 1);
        assert_eq!(plan.owners[0].adr_id, "ADR-0001");
        assert_eq!(
            plan.owners[0]
                .policies
                .iter()
                .map(|policy| policy.pattern_id.as_str())
                .collect::<Vec<_>>(),
            vec!["ADR-0001/functions", "ADR-0001/structs"]
        );
        assert_eq!(
            plan.owners[0].paths,
            BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()])
        );

        let _diagnostics = crate::check::validate_vault(&vault, None, &plan);
        assert_eq!(
            structural::work_counts().policy_compilations,
            2,
            "the check diagnostic adapter must reuse compiled plan outcomes"
        );

        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(structural::work_counts().policy_compilations, 2);
        assert_eq!(structural::work_counts().ast_parses, 2);
        let exact = violations[0]
            .location
            .as_ref()
            .unwrap()
            .lsp_range()
            .unwrap();
        assert_eq!((exact.start.line, exact.start.character), (0, 0));
        assert_eq!((exact.end.line, exact.end.character), (0, 12));
        assert_eq!(
            violations
                .iter()
                .map(|violation| (violation.pattern_id.as_str(), violation.path.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("ADR-0001/functions", "src/left.rs"),
                ("ADR-0001/functions", "src/right.rs"),
                ("ADR-0001/structs", "src/left.rs"),
                ("ADR-0001/structs", "src/right.rs"),
            ]
        );
    }

    #[test]
    fn changed_file_filter_preserves_policy_ids_and_limits_paths() {
        let (_temp, vault) = policy_fixture("two-policies");
        let plan = PolicyScanPlan::new(&vault);
        let changed = BTreeSet::from(["src/right.rs".to_string()]);

        let violations = plan.scan(&vault, Some(&changed)).unwrap();

        assert_eq!(
            violations
                .iter()
                .map(|violation| (violation.pattern_id.as_str(), violation.path.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("ADR-0001/functions", "src/right.rs"),
                ("ADR-0001/structs", "src/right.rs"),
            ]
        );
    }

    #[test]
    fn superseded_accepted_policies_are_compiled_for_diagnostics_but_not_scanned() {
        let (temp, _) = policy_fixture("two-policies");
        fs::copy(
            policy_fixture_root().join("mutations/0002-successor.md"),
            temp.path().join("docs/adr/0002-successor.md"),
        )
        .unwrap();
        let vault = Vault::load(temp.path()).unwrap();
        reset_work_counts();
        structural::reset_work_counts();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert!(plan.diagnostics.is_empty());
        assert_eq!(work_counts().definition_compilations, 2);
        assert_eq!(work_counts().adr_scope_resolutions, 0);
        assert!(plan.scan(&vault, None).unwrap().is_empty());
        assert_eq!(structural::work_counts().ast_parses, 0);
    }

    #[test]
    fn invalid_policy_reports_once_without_resolving_its_scope() {
        let (_temp, vault) = policy_fixture("invalid-policy");
        reset_work_counts();
        structural::reset_work_counts();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert_eq!(plan.diagnostics.len(), 1);
        assert!(matches!(
            plan.diagnostics[0].kind,
            PolicyDiagnosticKind::InvalidRule { .. }
        ));
        assert!(plan.state_definition_error().is_some());
        assert_eq!(
            work_counts(),
            WorkCounts {
                definition_compilations: 1,
                adr_scope_resolutions: 0,
            }
        );
        assert_eq!(structural::work_counts().policy_compilations, 1);
    }

    #[test]
    fn duplicate_ids_are_diagnosed_without_suppressing_executable_entries() {
        let (_temp, vault) = policy_fixture("duplicate-policies");

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(
            plan.diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic.kind,
                    PolicyDiagnosticKind::DuplicateId { .. }
                ))
                .count(),
            1
        );
        assert_eq!(plan.owners[0].policies.len(), 2);
        assert_eq!(
            plan.owners[0]
                .policies
                .iter()
                .filter(|policy| policy.state_pattern_id().is_some())
                .count(),
            1
        );
        assert_eq!(
            plan.owners[0].policies[0].state_pattern_id(),
            Some("ADR-0001/duplicate")
        );
        assert_eq!(plan.owners[0].policies[1].state_pattern_id(), None);
        assert_eq!(violations.len(), 4);
        assert!(
            violations
                .iter()
                .all(|violation| violation.pattern_id == "ADR-0001/duplicate")
        );
    }

    fn policy_fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policy-scan")
    }

    fn policy_fixture(name: &str) -> (TempDir, Vault) {
        let temp = TempDir::new().unwrap();
        copy_fixture_tree(&policy_fixture_root().join(name), temp.path()).unwrap();
        let vault = Vault::load(temp.path()).unwrap();
        (temp, vault)
    }
}

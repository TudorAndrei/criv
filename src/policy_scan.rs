use std::collections::BTreeSet;

use crate::Result;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::identity::pattern_id;
use crate::structural::{self, CompiledPolicy, PolicyScanRequest};
use crate::vault::{NoteKind, Vault};
#[cfg(test)]
use crate::work::{self, Work};

pub struct PolicyScanPlan {
    diagnostics: Vec<Diagnostic>,
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

impl PolicyScanPlan {
    #[expect(
        clippy::too_many_lines,
        reason = "policy planning collects related vault indexes in one deterministic pass"
    )]
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
                let definition_error = |code, message: String| {
                    Diagnostic::error(code, &note.rel_path, Some(policy.line), message)
                };
                let Some(local_id) = policy.id.as_deref() else {
                    if reports_diagnostics {
                        diagnostics.push(definition_error(
                            DiagnosticCode::MissingPolicyPatternId,
                            "policy pattern must declare an id".into(),
                        ));
                    }
                    continue;
                };

                let local_id = local_id.trim();
                if local_id.is_empty() {
                    if reports_diagnostics {
                        diagnostics.push(definition_error(
                            DiagnosticCode::EmptyPolicyPattern,
                            "policy pattern id may not be empty".into(),
                        ));
                    }
                    continue;
                }

                if reports_diagnostics && !ids.insert(local_id.to_string()) {
                    diagnostics.push(definition_error(
                        DiagnosticCode::DuplicatePolicyPattern,
                        format!("policy pattern id `{local_id}` is declared more than once"),
                    ));
                }

                #[cfg(test)]
                work::record(Work::PolicyDefinition);
                let full_id = active_adr_id.map(|adr_id| pattern_id(adr_id, local_id));
                let state_pattern_id = full_id.clone().filter(|full_id| {
                    vault.patterns().contains(full_id)
                        && vault.resolve_policy_pattern(full_id).is_some_and(
                            |(registered_note, registered_policy)| {
                                std::ptr::eq(registered_note, note)
                                    && std::ptr::eq(registered_policy, policy)
                            },
                        )
                });
                match structural::compile_policy(policy) {
                    Ok(compiled) => {
                        if let Some(pattern_id) = full_id {
                            candidates.push(CandidatePolicy {
                                pattern_id,
                                state_pattern_id,
                                fingerprint_material: format!("{policy:#?}"),
                                compiled,
                            });
                        }
                    }
                    Err(error) => {
                        let message = format!("policy pattern `{local_id}` {error}");
                        if reports_diagnostics {
                            diagnostics.push(definition_error(error.code(), message.clone()));
                        }
                        if let Some(pattern_id) = state_pattern_id {
                            state_definition_errors.push((pattern_id, message));
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
            work::record(Work::PolicyScopeResolution);
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

    pub(crate) fn definition_diagnostics(&self) -> &[Diagnostic] {
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
    ) -> Result<Vec<Diagnostic>> {
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
                violations.extend(rows.iter().map(|row| {
                    Diagnostic::policy_violation(
                        &row.path,
                        row.line,
                        row.location.clone(),
                        &owner.adr_id,
                        &policy.pattern_id,
                        &row.text,
                    )
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
        work::reset();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert!(plan.diagnostics.is_empty());
        assert_eq!(work::total(), 0);
    }

    #[test]
    fn owner_scope_and_compiled_policies_are_reused_by_the_batch() {
        let (_temp, vault) = policy_fixture("two-policies");
        work::reset();

        let plan = PolicyScanPlan::new(&vault);

        assert_eq!(
            work::counts([
                Work::PolicyDefinition,
                Work::PolicyScopeResolution,
                Work::PolicyCompilation,
            ]),
            [2, 1, 2]
        );
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

        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(
            work::counts([Work::PolicyCompilation, Work::AstParse]),
            [2, 2]
        );
        let exact = violations[0].location().unwrap().lsp_range().unwrap();
        assert_eq!((exact.start.line, exact.start.character), (0, 0));
        assert_eq!((exact.end.line, exact.end.character), (0, 12));
        assert_eq!(
            pattern_ids_and_paths(&violations),
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
            pattern_ids_and_paths(&violations),
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
        work::reset();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert!(plan.diagnostics.is_empty());
        assert_eq!(
            work::counts([Work::PolicyDefinition, Work::PolicyScopeResolution]),
            [2, 0]
        );
        assert!(plan.scan(&vault, None).unwrap().is_empty());
        assert_eq!(work::count(Work::AstParse), 0);
    }

    #[test]
    fn invalid_policy_reports_once_without_resolving_its_scope() {
        let (_temp, vault) = policy_fixture("invalid-policy");
        work::reset();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert_eq!(plan.diagnostics.len(), 1);
        assert_eq!(
            plan.diagnostics[0].code(),
            DiagnosticCode::InvalidPolicyPattern
        );
        assert!(plan.state_definition_error().is_some());
        assert_eq!(
            work::counts([
                Work::PolicyDefinition,
                Work::PolicyScopeResolution,
                Work::PolicyCompilation,
            ]),
            [1, 0, 1]
        );
    }

    #[test]
    fn duplicate_ids_are_diagnosed_without_suppressing_executable_entries() {
        let (_temp, vault) = policy_fixture("duplicate-policies");

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(
            plan.diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == DiagnosticCode::DuplicatePolicyPattern)
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
            pattern_ids_and_paths(&violations)
                .iter()
                .all(|(pattern_id, _)| *pattern_id == "ADR-0001/duplicate")
        );
    }

    #[test]
    fn padded_local_ids_name_one_pattern_in_state_links_and_violations() {
        let (temp, _) = policy_fixture("two-policies");
        let adr = temp.path().join("docs/adr/0001-policy.md");
        let contents = fs::read_to_string(&adr)
            .unwrap()
            .replace("- id: functions", "- id: \" functions \"");
        fs::write(&adr, contents).unwrap();
        let vault = Vault::load(temp.path()).unwrap();

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(
            plan.owners[0].policies[0].state_pattern_id(),
            Some("ADR-0001/functions")
        );
        assert_eq!(
            pattern_ids_and_paths(&violations)[0].0,
            "ADR-0001/functions"
        );
        assert_eq!(
            vault.resolve_link("match:ADR-0001/functions"),
            crate::vault::ResolvedLink::Pattern {
                id: "ADR-0001/functions".into()
            }
        );
    }

    fn pattern_ids_and_paths(violations: &[Diagnostic]) -> Vec<(&str, &str)> {
        violations
            .iter()
            .map(|violation| {
                let pattern_id = violation.message().split('`').nth(1).unwrap_or_default();
                (pattern_id, violation.path())
            })
            .collect()
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

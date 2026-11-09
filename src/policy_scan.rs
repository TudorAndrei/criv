use std::collections::{BTreeMap, BTreeSet};

use crate::Result;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::structural::{self, CompiledPolicy, PolicyScanRequest, StructuralMatch};
use crate::vault::{Note, PolicyPattern, Vault};
#[cfg(test)]
use crate::work::{self, Work};

/// The compiled registered policies of the effective decisions, with the
/// diagnostics about every decision's policy definitions.
pub struct PolicyScanPlan {
    diagnostics: Vec<Diagnostic>,
    owners: Vec<PlannedOwner>,
    registered_failures: Vec<PolicyFailure>,
}

pub struct PlannedOwner {
    adr_id: String,
    scopes: Vec<String>,
    paths: BTreeSet<String>,
    policies: Vec<PlannedPolicy>,
}

pub struct PlannedPolicy {
    pattern_id: String,
    definition: PolicyPattern,
    compiled: CompiledPolicy,
}

/// A registered pattern whose definition does not compile.
pub struct PolicyFailure {
    pattern_id: String,
    message: String,
}

impl PolicyScanPlan {
    pub(crate) fn new(vault: &Vault) -> Self {
        let mut diagnostics = duplicate_pattern_ids(vault);
        let mut owners: Vec<(&Note, &str, Vec<PlannedPolicy>)> = Vec::new();
        let mut registered_failures = Vec::new();

        for declaration in vault.policy_declarations() {
            let note = declaration.note;
            let policy = declaration.policy;
            let definition_error = |code, message: String| {
                Diagnostic::error(code, note.rel_path(), Some(policy.line), message)
            };
            let Some(local_id) = policy.id.as_deref() else {
                diagnostics.push(definition_error(
                    DiagnosticCode::MissingPolicyPatternId,
                    "policy pattern must declare an id".into(),
                ));
                continue;
            };
            let local_id = local_id.trim();
            if local_id.is_empty() {
                diagnostics.push(definition_error(
                    DiagnosticCode::EmptyPolicyPattern,
                    "policy pattern id may not be empty".into(),
                ));
                continue;
            }
            #[cfg(test)]
            work::record(Work::PolicyDefinition);
            let registered = declaration
                .id
                .zip(note.id())
                .filter(|_| declaration.registered);
            match structural::compile_policy(policy.definition()) {
                Ok(compiled) => {
                    if let Some((pattern_id, adr_id)) = registered {
                        let planned = PlannedPolicy {
                            pattern_id: pattern_id.to_string(),
                            definition: policy.clone(),
                            compiled,
                        };
                        match owners.last_mut() {
                            Some((owner, _, policies)) if owner.rel_path() == note.rel_path() => {
                                policies.push(planned);
                            }
                            _ => owners.push((note, adr_id, vec![planned])),
                        }
                    }
                }
                Err(error) => {
                    let message = format!("policy pattern `{local_id}` {error}");
                    diagnostics.push(definition_error(error.code(), message.clone()));
                    if let Some((pattern_id, _)) = registered {
                        registered_failures.push(PolicyFailure {
                            pattern_id: pattern_id.to_string(),
                            message,
                        });
                    }
                }
            }
        }

        let owners = owners
            .into_iter()
            .map(|(note, adr_id, policies)| {
                #[cfg(test)]
                work::record(Work::PolicyScopeResolution);
                PlannedOwner {
                    adr_id: adr_id.to_string(),
                    scopes: note.effective_governs().into_owned(),
                    paths: vault.governed_files(note).into_iter().collect(),
                    policies,
                }
            })
            .collect();
        registered_failures.sort_by(|left, right| left.pattern_id.cmp(&right.pattern_id));
        Self {
            diagnostics,
            owners,
            registered_failures,
        }
    }

    pub(crate) fn definition_diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Every full pattern id declared more than once: each later declaration in
/// the owning note, and every note when several notes declare it.
fn duplicate_pattern_ids(vault: &Vault) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut notes_by_id: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for declaration in vault.policy_declarations() {
        let Some(id) = declaration.id else {
            continue;
        };
        let note = declaration.note.rel_path();
        let notes = notes_by_id.entry(id).or_default();
        if !declaration.first && notes.first() == Some(&note) {
            let local_id = declaration.policy.id.as_deref().unwrap_or_default().trim();
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::DuplicatePolicyPattern,
                note,
                Some(declaration.policy.line),
                format!("policy pattern id `{local_id}` is declared more than once"),
            ));
        }
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    for (pattern, notes) in notes_by_id {
        if notes.len() <= 1 {
            continue;
        }
        for note in &notes {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::DuplicatePatternId,
                note,
                None,
                format!(
                    "pattern id `{pattern}` is declared in multiple notes: {}",
                    notes.join(", ")
                ),
            ));
        }
    }
    diagnostics
}

impl PolicyScanPlan {
    pub(crate) fn owners(&self) -> &[PlannedOwner] {
        &self.owners
    }

    pub(crate) fn registered_failures(&self) -> &[PolicyFailure] {
        &self.registered_failures
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
                changed_files.map_or_else(
                    || owner.paths.clone(),
                    |changed| owner.paths.intersection(changed).cloned().collect(),
                )
            })
            .collect::<Vec<_>>();

        let mut scanned = Vec::new();
        let mut requests = Vec::new();
        for (owner, paths) in self.owners.iter().zip(&scan_paths) {
            for policy in &owner.policies {
                scanned.push((owner, policy));
                requests.push(PolicyScanRequest {
                    policy: &policy.compiled,
                    paths,
                });
            }
        }

        let rows = structural::find_policies_batch(vault.repository_files(), &requests)?;
        Ok(scanned
            .into_iter()
            .zip(rows)
            .flat_map(|((owner, policy), rows)| {
                rows.into_iter()
                    .map(|row| policy_violation(&owner.adr_id, &policy.pattern_id, row))
            })
            .collect())
    }
}

fn policy_violation(adr_id: &str, pattern_id: &str, row: StructuralMatch) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::PolicyViolation,
        &row.path,
        Some(row.line),
        format!("{adr_id} policy `{pattern_id}` matched `{}`", row.text),
    )
    .at(row.location)
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
    pub(crate) fn pattern_id(&self) -> &str {
        &self.pattern_id
    }

    pub(crate) const fn definition(&self) -> &PolicyPattern {
        &self.definition
    }

    pub(crate) const fn compiled(&self) -> &CompiledPolicy {
        &self.compiled
    }
}

impl PolicyFailure {
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;
    use crate::repository::RepositoryFiles;
    use crate::test_support::copy_fixture_tree;
    use crate::vault::LoadOptions;

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
                .map(PlannedPolicy::pattern_id)
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
            paths_and_messages(&violations),
            [
                (
                    "src/left.rs",
                    "ADR-0001 policy `ADR-0001/functions` matched `fn left() {}`"
                ),
                (
                    "src/right.rs",
                    "ADR-0001 policy `ADR-0001/functions` matched `fn right() {}`"
                ),
                (
                    "src/left.rs",
                    "ADR-0001 policy `ADR-0001/structs` matched `struct Left;`"
                ),
                (
                    "src/right.rs",
                    "ADR-0001 policy `ADR-0001/structs` matched `struct Right;`"
                ),
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
            paths_and_messages(&violations),
            [
                (
                    "src/right.rs",
                    "ADR-0001 policy `ADR-0001/functions` matched `fn right() {}`"
                ),
                (
                    "src/right.rs",
                    "ADR-0001 policy `ADR-0001/structs` matched `struct Right;`"
                ),
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
        let vault = load(temp.path());
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
        assert_eq!(plan.registered_failures().len(), 1);
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
    fn every_registered_failure_is_kept_in_pattern_id_order() {
        let (temp, _) = policy_fixture("invalid-policy");
        let adr = temp.path().join("docs/adr/0001-policy.md");
        let contents = fs::read_to_string(&adr)
            .unwrap()
            .replace("\r\n", "\n")
            .replace(
                "policy:\n  patterns:\n",
                "policy:\n  patterns:\n    - id: also-invalid\n      language: rust\n",
            );
        fs::write(&adr, contents).unwrap();
        let vault = load(temp.path());

        let plan = PolicyScanPlan::new(&vault);

        assert_eq!(
            plan.registered_failures()
                .iter()
                .map(|failure| failure.pattern_id.as_str())
                .collect::<Vec<_>>(),
            ["ADR-0001/also-invalid", "ADR-0001/invalid"]
        );
    }

    #[test]
    fn duplicate_ids_register_and_scan_only_the_first_declaration() {
        let (_temp, vault) = policy_fixture("duplicate-policies");

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(
            plan.diagnostics
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            [DiagnosticCode::DuplicatePolicyPattern]
        );
        assert_eq!(
            plan.owners[0]
                .policies
                .iter()
                .map(PlannedPolicy::pattern_id)
                .collect::<Vec<_>>(),
            ["ADR-0001/duplicate"]
        );
        assert_eq!(
            paths_and_messages(&violations),
            [
                (
                    "src/left.rs",
                    "ADR-0001 policy `ADR-0001/duplicate` matched `fn left() {}`"
                ),
                (
                    "src/right.rs",
                    "ADR-0001 policy `ADR-0001/duplicate` matched `fn right() {}`"
                ),
            ]
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
        let vault = load(temp.path());

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(&vault, None).unwrap();

        assert_eq!(
            plan.owners[0].policies[0].pattern_id(),
            "ADR-0001/functions"
        );
        assert_eq!(
            violations[0].message(),
            "ADR-0001 policy `ADR-0001/functions` matched `fn left() {}`"
        );
        assert!(vault.resolve_policy_pattern("ADR-0001/functions").is_some());
    }

    fn paths_and_messages(violations: &[Diagnostic]) -> Vec<(&str, &str)> {
        violations
            .iter()
            .map(|violation| (violation.path(), violation.message()))
            .collect()
    }

    fn policy_fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policy-scan")
    }

    fn load(root: &Path) -> Vault {
        Vault::load(
            &RepositoryFiles::open(root).unwrap(),
            LoadOptions::default(),
        )
        .unwrap()
    }

    fn policy_fixture(name: &str) -> (TempDir, Vault) {
        let temp = TempDir::new().unwrap();
        copy_fixture_tree(&policy_fixture_root().join(name), temp.path());
        let vault = load(temp.path());
        (temp, vault)
    }
}

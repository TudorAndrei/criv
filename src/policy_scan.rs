use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::Result;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::identity::pattern_id;
use crate::structural::{self, CompiledPolicy, PolicyPattern, PolicyScanRequest};
use crate::vault::{NoteKind, Vault};
#[cfg(test)]
use crate::work::{self, Work};

/// The compiled inline policies of the effective decisions, with the
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
    registered: bool,
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
        let mut diagnostics = Vec::new();
        let mut owners = Vec::new();
        let mut registered_failures = Vec::new();

        for note in vault
            .notes()
            .iter()
            .filter(|note| note.kind() == NoteKind::Decision)
        {
            let active_adr_id = vault
                .is_effective_decision(note)
                .then(|| note.id())
                .flatten();
            let mut ids = BTreeSet::new();
            let mut policies = Vec::new();
            for policy in note.policy_patterns() {
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

                let first_declaration = ids.insert(local_id.to_string());
                if !first_declaration {
                    diagnostics.push(definition_error(
                        DiagnosticCode::DuplicatePolicyPattern,
                        format!("policy pattern id `{local_id}` is declared more than once"),
                    ));
                }

                #[cfg(test)]
                work::record(Work::PolicyDefinition);
                let full_id = active_adr_id.map(|adr_id| pattern_id(adr_id, local_id));
                // A registered pattern id names the first declaration of the
                // local id in the decision note that the ADR id resolves to.
                let registered = first_declaration
                    && full_id.as_ref().is_some_and(|full_id| {
                        vault.patterns().contains(full_id)
                            && vault
                                .resolve_policy_pattern(full_id)
                                .is_some_and(|(owner, _)| owner.rel_path() == note.rel_path())
                    });
                match structural::compile_policy(policy) {
                    Ok(compiled) => {
                        if let Some(pattern_id) = full_id {
                            policies.push(PlannedPolicy {
                                pattern_id,
                                registered,
                                definition: policy.clone(),
                                compiled,
                            });
                        }
                    }
                    Err(error) => {
                        let message = format!("policy pattern `{local_id}` {error}");
                        diagnostics.push(definition_error(error.code(), message.clone()));
                        if let (true, Some(pattern_id)) = (registered, full_id) {
                            registered_failures.push(PolicyFailure {
                                pattern_id,
                                message,
                            });
                        }
                    }
                }
            }

            let Some(adr_id) = active_adr_id else {
                continue;
            };
            if policies.is_empty() {
                continue;
            }

            #[cfg(test)]
            work::record(Work::PolicyScopeResolution);
            owners.push(PlannedOwner {
                adr_id: adr_id.to_string(),
                scopes: note.effective_governs().into_owned(),
                paths: vault.governed_files(note).into_iter().collect(),
                policies,
            });
        }

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

    pub(crate) fn owners(&self) -> &[PlannedOwner] {
        &self.owners
    }

    /// Every registered pattern whose definition does not compile, by id.
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
                changed_files.map_or(Cow::Borrowed(&owner.paths), |changed| {
                    Cow::Owned(owner.paths.intersection(changed).cloned().collect())
                })
            })
            .collect::<Vec<_>>();

        let mut records = Vec::new();
        let mut requests = Vec::new();
        for (owner, paths) in self.owners.iter().zip(&scan_paths) {
            for policy in &owner.policies {
                let key = records.len();
                records.push((owner, policy));
                requests.push(PolicyScanRequest {
                    key,
                    policy: &policy.compiled,
                    paths,
                });
            }
        }

        let rows_by_key = structural::find_policies_batch(
            vault.repository_files(),
            vault.source_files(),
            &requests,
        )?;
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
    pub(crate) fn pattern_id(&self) -> &str {
        &self.pattern_id
    }

    /// Whether this declaration is the one its pattern id registers.
    pub(crate) const fn is_registered(&self) -> bool {
        self.registered
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
    use crate::identity::copy_fixture_tree;
    use crate::repository::RepositoryFiles;
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
        let contents = fs::read_to_string(&adr).unwrap().replace(
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
    fn duplicate_ids_register_only_the_first_declaration() {
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
        assert_eq!(
            plan.owners[0]
                .policies
                .iter()
                .map(|policy| (policy.pattern_id(), policy.is_registered()))
                .collect::<Vec<_>>(),
            [("ADR-0001/duplicate", true), ("ADR-0001/duplicate", false)]
        );
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
        let vault = load(temp.path());

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(&vault, None).unwrap();

        let first = &plan.owners[0].policies[0];
        assert_eq!(
            (first.pattern_id(), first.is_registered()),
            ("ADR-0001/functions", true)
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

    fn load(root: &Path) -> Vault {
        Vault::load(
            &RepositoryFiles::open(root).unwrap(),
            LoadOptions::default(),
        )
        .unwrap()
    }

    fn policy_fixture(name: &str) -> (TempDir, Vault) {
        let temp = TempDir::new().unwrap();
        copy_fixture_tree(&policy_fixture_root().join(name), temp.path()).unwrap();
        let vault = load(temp.path());
        (temp, vault)
    }
}

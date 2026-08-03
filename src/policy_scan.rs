use std::collections::BTreeSet;
use std::path::Path;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use crate::Result;
use crate::structural::{self, CompiledPolicy, PolicyCompileError, PolicyScanRequest};
use crate::vault::{NoteKind, Vault};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum PolicyDiagnosticKind {
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
pub(crate) struct PolicyDiagnostic {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) kind: PolicyDiagnosticKind,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PolicyViolation {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) adr_id: String,
    pub(crate) pattern_id: String,
    pub(crate) text: String,
}

pub(crate) struct PolicyScanPlan {
    diagnostics: Vec<PolicyDiagnostic>,
    owners: Vec<PlannedOwner>,
}

struct PlannedOwner {
    adr_id: String,
    paths: BTreeSet<String>,
    policies: Vec<PlannedPolicy>,
}

struct PlannedPolicy {
    pattern_id: String,
    compiled: CompiledPolicy,
}

enum ScanPaths<'a> {
    Borrowed(&'a BTreeSet<String>),
    Filtered(BTreeSet<String>),
}

impl ScanPaths<'_> {
    fn as_set(&self) -> &BTreeSet<String> {
        match self {
            Self::Borrowed(paths) => paths,
            Self::Filtered(paths) => paths,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    definition_compilations: usize,
    adr_scope_resolutions: usize,
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
fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

impl PolicyScanPlan {
    pub(crate) fn new(vault: &Vault) -> Self {
        let mut diagnostics = Vec::new();
        let mut owners = Vec::new();

        for note in &vault.notes {
            let reports_diagnostics = note.kind == NoteKind::Decision;
            let active_adr_id = (note.status.as_deref() == Some("accepted"))
                .then_some(note.id.as_deref())
                .flatten();
            if !reports_diagnostics && active_adr_id.is_none() {
                continue;
            }

            let mut ids = BTreeSet::new();
            let mut policies = Vec::new();
            for policy in &note.policy_patterns {
                let Some(local_id) = policy.id.as_deref() else {
                    if reports_diagnostics {
                        diagnostics.push(PolicyDiagnostic {
                            path: note.rel_path.clone(),
                            line: policy.line,
                            kind: PolicyDiagnosticKind::MissingId,
                        });
                    }
                    continue;
                };

                let local_id = local_id.trim();
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
                record_work(|counts| counts.definition_compilations += 1);
                match structural::compile_policy(policy) {
                    Ok(compiled) => {
                        if let Some(adr_id) = active_adr_id {
                            policies.push(PlannedPolicy {
                                pattern_id: format!("{adr_id}/{local_id}"),
                                compiled,
                            });
                        }
                    }
                    Err(error) if reports_diagnostics => {
                        diagnostics.push(PolicyDiagnostic {
                            path: note.rel_path.clone(),
                            line: policy.line,
                            kind: diagnostic_kind(local_id, error),
                        });
                    }
                    Err(_) => {}
                }
            }

            let Some(adr_id) = active_adr_id else {
                continue;
            };
            if policies.is_empty() {
                continue;
            }

            #[cfg(test)]
            record_work(|counts| counts.adr_scope_resolutions += 1);
            let paths = vault
                .source_files_matching_globs(&vault.effective_governs(note))
                .into_iter()
                .collect();
            owners.push(PlannedOwner {
                adr_id: adr_id.to_string(),
                paths,
                policies,
            });
        }

        Self {
            diagnostics,
            owners,
        }
    }

    pub(crate) fn definition_diagnostics(&self) -> &[PolicyDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn scan(
        &self,
        root: &Path,
        vault: &Vault,
        changed_files: Option<&BTreeSet<String>>,
    ) -> Result<Vec<PolicyViolation>> {
        let scan_paths = self
            .owners
            .iter()
            .map(|owner| match changed_files {
                None => ScanPaths::Borrowed(&owner.paths),
                Some(changed) => ScanPaths::Filtered(
                    owner
                        .paths
                        .iter()
                        .filter(|path| changed.contains(*path))
                        .cloned()
                        .collect(),
                ),
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
                    paths: scan_paths[owner_index].as_set(),
                });
            }
        }

        let rows_by_key = structural::find_policies_batch(root, vault, &requests)?;
        let mut violations = Vec::new();
        for (key, (owner, policy)) in records.into_iter().enumerate() {
            if let Some(rows) = rows_by_key.get(&key) {
                violations.extend(rows.iter().map(|row| PolicyViolation {
                    path: row.path.clone(),
                    line: row.line,
                    adr_id: owner.adr_id.clone(),
                    pattern_id: policy.pattern_id.clone(),
                    text: row.text.clone(),
                }));
            }
        }
        Ok(violations)
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

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn policy_free_accepted_adrs_resolve_no_policy_scopes() {
        let (_temp, vault) = policy_fixture(
            r#"---
id: ADR-0001
kind: decision
title: No policy
status: accepted
governs:
  - src/**
---

# No policy
"#,
        );
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
        let (temp, vault) = policy_fixture(&two_policy_adr());
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

        let _diagnostics = crate::check::validate_with_policy_plan(&vault, &plan);
        assert_eq!(
            structural::work_counts().policy_compilations,
            2,
            "the check diagnostic adapter must reuse compiled plan outcomes"
        );

        let violations = plan.scan(temp.path(), &vault, None).unwrap();

        assert_eq!(structural::work_counts().policy_compilations, 2);
        assert_eq!(structural::work_counts().ast_parses, 2);
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
        let (temp, vault) = policy_fixture(&two_policy_adr());
        let plan = PolicyScanPlan::new(&vault);
        let changed = BTreeSet::from(["src/right.rs".to_string()]);

        let violations = plan.scan(temp.path(), &vault, Some(&changed)).unwrap();

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
    fn invalid_policy_reports_once_without_resolving_its_scope() {
        let (_temp, vault) = policy_fixture(
            r#"---
id: ADR-0001
kind: decision
title: Invalid policy
status: accepted
governs:
  - src/**
policy:
  patterns:
    - id: invalid
      language: rust
      rule: "not: [valid"
---

# Invalid policy
"#,
        );
        reset_work_counts();
        structural::reset_work_counts();

        let plan = PolicyScanPlan::new(&vault);

        assert!(plan.owners.is_empty());
        assert_eq!(plan.diagnostics.len(), 1);
        assert!(matches!(
            plan.diagnostics[0].kind,
            PolicyDiagnosticKind::InvalidRule { .. }
        ));
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
        let (temp, vault) = policy_fixture(
            r#"---
id: ADR-0001
kind: decision
title: Duplicate policies
status: accepted
governs:
  - src/**
policy:
  patterns:
    - id: duplicate
      language: rust
      pattern: "fn $NAME() { $$$ }"
    - id: duplicate
      language: rust
      pattern: "struct $NAME;"
---

# Duplicate policies
"#,
        );

        let plan = PolicyScanPlan::new(&vault);
        let violations = plan.scan(temp.path(), &vault, None).unwrap();

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
        assert_eq!(violations.len(), 4);
        assert!(
            violations
                .iter()
                .all(|violation| violation.pattern_id == "ADR-0001/duplicate")
        );
    }

    fn two_policy_adr() -> String {
        r#"---
id: ADR-0001
kind: decision
title: Two policies
status: accepted
governs:
  - src/**
policy:
  patterns:
    - id: functions
      language: rust
      pattern: "fn $NAME() { $$$ }"
    - id: structs
      language: rust
      pattern: "struct $NAME;"
---

# Two policies
"#
        .to_string()
    }

    fn policy_fixture(adr: &str) -> (TempDir, Vault) {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/left.rs"), "fn left() {}\nstruct Left;\n").unwrap();
        fs::write(root.join("src/right.rs"), "fn right() {}\nstruct Right;\n").unwrap();
        fs::write(root.join("docs/adr/0001-policy.md"), adr).unwrap();
        let vault = Vault::load(root).unwrap();
        (temp, vault)
    }
}

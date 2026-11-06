use std::collections::BTreeSet;
use std::env;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use usage_rs::{Args as UsageArgs, ValueEnum};

use crate::check;
use crate::config::{CONFIG_FILE, ImportPolicyConfig, Stage};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::git::{ChangedSet, Comparison, GitRepository, is_object_id, is_zero_oid};
use crate::glob::GlobMatcher;
use crate::vault::{LoadOptions, Vault};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, UsageArgs)]
pub struct EnforceOptions {
    #[usage(long, value_enum)]
    stage: Stage,
    /// Select the human report or a JSON enforcement report.
    #[usage(long, value_enum, default = "text")]
    format: Format,
    /// Consume Git's pre-push ref-update records from standard input.
    #[usage(long, hide)]
    pre_push: bool,
    #[usage(long, hide, requires = "--pre-push", required_if = "--pre-push")]
    remote_name: Option<String>,
}

/// Validate the selected Git transaction and report stage-specific failures.
pub fn run(root: &Path, options: &EnforceOptions) -> Result<()> {
    if options.pre_push && options.stage != Stage::Push {
        return Err(CrivError::usage(
            "error: --pre-push is only valid with --stage push",
        ));
    }
    let files = crate::config::open_vault(root)?;
    let vault = Vault::load(&files, LoadOptions::default())?;
    if !vault.config().enforce_stages().contains(&options.stage) {
        return Err(CrivError::new(format!(
            "stage `{}` is not enabled in {CONFIG_FILE}",
            options.stage.as_str()
        )));
    }
    let repository = GitRepository::discover(root)?;
    let Selection {
        changes,
        ci_base_ref,
    } = changed_entries(repository.as_ref(), options)?;
    let identity_conflicts = match (&repository, ci_base_ref) {
        (Some(repository), Some(base_ref)) => {
            crate::adr::identity_conflict(&files, repository, &base_ref)?
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    };

    let check::Validation {
        diagnostics,
        policies,
    } = check::validate_vault(&vault);
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();

    let changed_count = changes
        .as_ref()
        .map_or(0, |changes| changes.affected_paths().len());
    let scanned_files = changes
        .as_ref()
        .filter(|_| options.stage != Stage::Ci)
        .and_then(|changes| check::changed_scope(changes, vault.config()));
    let policy_violations = policies
        .scan(&vault, scanned_files.as_ref())?
        .iter()
        .map(Diagnostic::describe)
        .collect();
    let import_violations = import_policy_violations(&vault, scanned_files.as_ref())?;
    let adr_violations = crate::adr::change_violations(
        &files,
        repository.as_ref(),
        vault.config(),
        changes.as_ref(),
        options.stage,
    );
    let failures = Failure::in_priority_order(
        identity_conflicts,
        policy_violations,
        import_violations,
        adr_violations,
        errors,
    );
    let report = EnforceReport {
        stage: options.stage.as_str(),
        ok: failures.is_empty(),
        errors,
        warnings,
        changed_files: changed_count,
        basis: changes
            .as_ref()
            .map_or("no comparison", |changes| changes.basis.as_str()),
        violations: failures
            .iter()
            .flat_map(|failure| failure.violations.iter().cloned())
            .collect(),
        codes: failures.iter().map(|failure| failure.code).collect(),
        code: failures.first().map(|failure| failure.code),
        fix: failures.first().and_then(|failure| failure.code.fix()),
    };
    report.print(options.format, options.stage)?;
    report.code.map_or(Ok(()), |code| {
        Err(CrivError::coded(
            code,
            failures
                .iter()
                .map(|failure| failure.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ))
    })
}

impl EnforceReport<'_> {
    fn print(&self, format: Format, stage: Stage) -> Result<()> {
        if format == Format::Json {
            let json = serde_json::to_string_pretty(self).map_err(|err| {
                CrivError::new(format!("failed to serialize enforcement report: {err}"))
            })?;
            println!("{json}");
            return Ok(());
        }
        let Self {
            errors,
            warnings,
            changed_files,
            basis,
            ..
        } = self;
        // CI validates the whole vault and reports no count.
        let files = match stage {
            Stage::Commit => format!(", {changed_files} staged files"),
            Stage::Push => format!(", {changed_files} changed files"),
            Stage::Ci => String::new(),
        };
        println!(
            "{} enforcement: {errors} validation errors, {warnings} warnings{files} ({basis})",
            self.stage
        );
        for violation in &self.violations {
            println!("{violation}");
        }
        if self.ok {
            println!("enforcement passed");
        }
        Ok(())
    }
}

/// One failing category of an enforcement run.
struct Failure {
    code: DiagnosticCode,
    message: String,
    violations: Vec<String>,
}

impl Failure {
    /// Every failing category; the first sets the exit status.
    fn in_priority_order(
        identity_conflicts: Vec<String>,
        policy_violations: Vec<String>,
        import_violations: Vec<String>,
        adr_violations: Vec<String>,
        errors: usize,
    ) -> Vec<Self> {
        [
            Self::new(
                DiagnosticCode::AdrIdConflict,
                "ADR ID conflict",
                identity_conflicts,
            ),
            Self::new(
                DiagnosticCode::PolicyViolation,
                "policy violation",
                policy_violations,
            ),
            Self::new(
                DiagnosticCode::ImportPolicyViolation,
                "import policy violation",
                import_violations,
            ),
            Self::new(
                DiagnosticCode::AdrImmutabilityViolation,
                "ADR immutability violation",
                adr_violations,
            ),
        ]
        .into_iter()
        .flatten()
        .chain((errors > 0).then(|| Self {
            code: DiagnosticCode::EnforcementFailed,
            message: "enforcement failed".into(),
            violations: Vec::new(),
        }))
        .collect()
    }

    /// The category of `violations`, or `None` when it has none.
    fn new(code: DiagnosticCode, noun: &str, violations: Vec<String>) -> Option<Self> {
        (!violations.is_empty()).then(|| Self {
            code,
            message: format!("{} {noun}(s) found", violations.len()),
            violations,
        })
    }
}

#[derive(Serialize)]
struct EnforceReport<'a> {
    stage: &'a str,
    ok: bool,
    errors: usize,
    warnings: usize,
    changed_files: usize,
    basis: &'a str,
    /// Every violation, grouped by failing category in `codes` order.
    violations: Vec<String>,
    /// Every failing category, highest priority first.
    codes: Vec<DiagnosticCode>,
    /// The highest-priority failing category, which sets the exit status.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<DiagnosticCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<&'static str>,
}

fn import_policy_violations(
    vault: &Vault,
    changed_files: Option<&BTreeSet<String>>,
) -> Result<Vec<String>> {
    let mut violations = Vec::new();
    for config in vault.config().import_policies() {
        let policy = ImportPolicy::compile(config)?;
        for file in vault.source_graph().files() {
            if changed_files.is_some_and(|files| !files.contains(&file.path)) {
                continue;
            }
            if !policy.scope.is_match(&file.path) {
                continue;
            }
            for import in &file.imports {
                if policy.deny.iter().any(|rule| rule.matches(&import.module)) {
                    violations.push(format!(
                        "{}:{}: import policy `{}` denies `{}`",
                        file.path, import.line, config.id, import.module
                    ));
                }
            }
        }
    }
    violations.sort();
    violations.dedup();
    Ok(violations)
}

/// A configured import policy with its globs compiled.
struct ImportPolicy {
    scope: GlobMatcher,
    deny: Vec<DenyRule>,
}

impl ImportPolicy {
    fn compile(config: &ImportPolicyConfig) -> Result<Self> {
        let id = &config.id;
        let scope = GlobMatcher::new(&config.scope).map_err(|err| {
            CrivError::new(format!(
                "invalid enforce.imports scope for policy `{id}`: {err}"
            ))
        })?;
        let deny = config
            .deny
            .iter()
            .map(|pattern| DenyRule::new(pattern, id))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { scope, deny })
    }
}

/// One denied import: a module path, or a glob over `/`-separated module paths.
struct DenyRule {
    pattern: String,
    matcher: Option<GlobMatcher>,
}

impl DenyRule {
    fn new(pattern: &str, policy_id: &str) -> Result<Self> {
        let matcher = pattern
            .contains(['*', '?', '['])
            .then(|| {
                GlobMatcher::new(&[pattern.replace("::", "/")]).map_err(|err| {
                    CrivError::new(format!(
                        "invalid enforce.imports deny glob for policy `{policy_id}`: {err}"
                    ))
                })
            })
            .transpose()?;
        Ok(Self {
            pattern: pattern.to_string(),
            matcher,
        })
    }

    /// A module path matches the exact path, any path below it, or the glob.
    fn matches(&self, module: &str) -> bool {
        self.matcher.as_ref().map_or_else(
            || {
                module
                    .strip_prefix(self.pattern.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
            },
            |matcher| matcher.is_match(&module.replace("::", "/")),
        )
    }
}

/// The changes a stage selects and, in CI, the base ref they compare with.
#[derive(Debug)]
struct Selection {
    changes: Option<ChangedSet>,
    ci_base_ref: Option<String>,
}

impl Selection {
    const fn of(changes: Option<ChangedSet>) -> Self {
        Self {
            changes,
            ci_base_ref: None,
        }
    }
}

fn changed_entries(
    repository: Option<&GitRepository>,
    options: &EnforceOptions,
) -> Result<Selection> {
    match options.stage {
        Stage::Commit => repository
            .map(|repo| repo.changed_set(&Comparison::Staged, &[]))
            .transpose()
            .map(Selection::of),
        Stage::Push => match options.remote_name.as_deref() {
            Some(remote_name) => pre_push_changed_entries(
                required_repository(repository)?,
                remote_name,
                &read_pre_push_updates()?,
            )
            .map(|changes| Selection::of(Some(changes))),
            None => repository
                .map(manual_push_changed_set)
                .transpose()
                .map(Selection::of),
        },
        Stage::Ci => ci_changed_entries(
            repository,
            env::var("CRIV_BASE_REF").ok().as_deref(),
            env::var("GITHUB_BASE_REF").ok(),
            is_ci_environment(),
        ),
    }
}

/// A manual push compares with the upstream branch. Without one it falls
/// back to the last commit and names the reason in its basis; generated
/// hooks use the complete pre-push input instead.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "`@{upstream}` is Git revision syntax"
)]
fn manual_push_changed_set(repository: &GitRepository) -> Result<ChangedSet> {
    let upstream = match repository.changed_set(
        &Comparison::SinceMergeBase {
            upstream_ref: "@{upstream}",
        },
        &[],
    ) {
        Ok(changes) => return Ok(changes),
        Err(error) => error,
    };
    let mut changes = repository.changed_set(
        &Comparison::Trees {
            old_ref: "HEAD~1",
            new_ref: "HEAD",
        },
        &[],
    )?;
    changes.basis = format!("{}; no upstream comparison: {upstream}", changes.basis);
    Ok(changes)
}

/// CI compares `HEAD` with `CRIV_BASE_REF`, else with the first of
/// `origin/$GITHUB_BASE_REF` and `$GITHUB_BASE_REF` that Git can compare.
/// Outside CI, a missing base falls back to the worktree with no base ref.
fn ci_changed_entries(
    repository: Option<&GitRepository>,
    criv_base_ref: Option<&str>,
    github_base_ref: Option<String>,
    ci_environment: bool,
) -> Result<Selection> {
    let against = |repository: &GitRepository, base_ref: String| {
        let changes = repository.changed_set(
            &Comparison::Trees {
                old_ref: &base_ref,
                new_ref: "HEAD",
            },
            &[],
        )?;
        Ok(Selection {
            changes: Some(changes),
            ci_base_ref: Some(base_ref),
        })
    };
    if let Some(base_ref) = criv_base_ref {
        return against(required_repository(repository)?, base_ref.to_owned());
    }

    let mut failures = Vec::new();
    if let Some(base_ref) = github_base_ref
        && let Some(repository) = repository
    {
        for old_ref in [format!("origin/{base_ref}"), base_ref] {
            match against(repository, old_ref) {
                Ok(selection) => return Ok(selection),
                Err(error) => failures.push(error.to_string()),
            }
        }
    }
    let causes = if failures.is_empty() {
        String::new()
    } else {
        format!("; {}", failures.join("; "))
    };

    if ci_environment {
        return Err(CrivError::new(format!(
            "ci enforcement requires CRIV_BASE_REF or a fetchable GITHUB_BASE_REF{causes}"
        )));
    }

    repository
        .map(|repo| {
            let mut changes = repo.changed_set(&Comparison::Worktree { old_ref: "HEAD" }, &[])?;
            changes.basis.push_str(&causes);
            Ok(changes)
        })
        .transpose()
        .map(Selection::of)
}

fn required_repository(repository: Option<&GitRepository>) -> Result<&GitRepository> {
    repository.ok_or_else(|| CrivError::new("git diff failed: not inside a Git worktree"))
}

fn is_ci_environment() -> bool {
    env::var("CI").is_ok_and(|value| value == "true")
        || env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
}

/// One record of Git's pre-push input: the local and remote object IDs.
#[derive(Debug, Clone, Eq, PartialEq)]
struct PrePushUpdate {
    local_oid: String,
    remote_oid: String,
}

fn read_pre_push_updates() -> Result<Vec<PrePushUpdate>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|err| {
        CrivError::new(format!("failed to read pre-push updates from stdin: {err}"))
    })?;
    parse_pre_push_updates(&input)
}

fn parse_pre_push_updates(input: &str) -> Result<Vec<PrePushUpdate>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let [_local_ref, local_oid, _remote_ref, remote_oid] = fields.as_slice() else {
                return Err(CrivError::new(format!(
                    "invalid pre-push ref update `{line}`; expected local-ref local-oid remote-ref remote-oid"
                )));
            };
            for oid in [*local_oid, *remote_oid] {
                if !is_object_id(oid) {
                    return Err(CrivError::new(format!(
                        "invalid pre-push object ID `{oid}`"
                    )));
                }
            }
            Ok(PrePushUpdate {
                local_oid: (*local_oid).to_string(),
                remote_oid: (*remote_oid).to_string(),
            })
        })
        .collect()
}

fn pre_push_changed_entries(
    repository: &GitRepository,
    remote_name: &str,
    updates: &[PrePushUpdate],
) -> Result<ChangedSet> {
    let mut entries = Vec::new();
    for update in updates {
        if is_zero_oid(&update.local_oid) {
            continue;
        }
        for commit in
            repository.outgoing_commits(remote_name, &update.local_oid, &update.remote_oid)?
        {
            entries.extend(repository.changed_set_for_commit(&commit)?.entries);
        }
    }
    Ok(ChangedSet {
        entries,
        old_ref: None,
        new: None,
        basis: format!("pre-push ref updates for remote {remote_name}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::ChangeStatus;
    use crate::git::fixture::git;

    #[test]
    fn deny_rules_match_exact_prefix_and_glob_forms() {
        let rule = |pattern: &str| DenyRule::new(pattern, "test").unwrap();
        assert!(rule("crate::infra").matches("crate::infra::db"));
        assert!(rule("crate::infra::*").matches("crate::infra::db"));
        assert!(rule("sqlx").matches("sqlx"));
        assert!(!rule("crate::infra").matches("crate::infrastructure"));
    }

    #[test]
    fn rejects_invalid_import_policy_scope_and_deny_globs() {
        let policy = |id: &str, scope: &str, deny: &str| ImportPolicyConfig {
            id: id.into(),
            scope: vec![scope.into()],
            deny: vec![deny.into()],
        };
        let scope = ImportPolicy::compile(&policy("bad-scope", "src/[.rs", "crate::db"))
            .err()
            .unwrap()
            .to_string();
        assert!(scope.contains("bad-scope") && scope.contains("scope"));

        let deny = ImportPolicy::compile(&policy("bad-deny", "src/**", "crate::["))
            .err()
            .unwrap()
            .to_string();
        assert!(deny.contains("bad-deny") && deny.contains("deny glob"));
    }

    fn two_commit_repository() -> tempfile::TempDir {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        std::fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-m", "initial"]);
        std::fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-m", "change"]);
        root
    }

    #[test]
    fn parses_multiple_pre_push_updates_and_rejects_malformed_records() {
        let updates = parse_pre_push_updates(
            "refs/heads/main 1111111111111111111111111111111111111111 refs/heads/main 0000000000000000000000000000000000000000\nrefs/heads/topic 2222222222222222222222222222222222222222 refs/heads/topic 3333333333333333333333333333333333333333\n",
        )
        .unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[1].remote_oid, "3".repeat(40));
        assert!(parse_pre_push_updates("broken\n").is_err());
        assert!(
            parse_pre_push_updates("refs/heads/main 1111111 refs/heads/main 2222222\n").is_err()
        );
    }

    #[test]
    fn pre_push_checks_each_outgoing_commit_on_a_new_branch() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        std::fs::create_dir_all(root.path().join("docs/adr")).unwrap();
        std::fs::write(root.path().join("docs/adr/0001-test.md"), "first\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "add adr"]);
        std::fs::write(root.path().join("docs/adr/0001-test.md"), "changed\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "modify adr"]);

        let changes = pre_push_changed_entries(
            &GitRepository::open(root.path()).unwrap(),
            "origin",
            &[PrePushUpdate {
                local_oid: git(root.path(), &["rev-parse", "HEAD"]),
                remote_oid: "0".repeat(40),
            }],
        )
        .unwrap();

        assert_eq!(
            changes
                .entries
                .iter()
                .map(|entry| (entry.status, entry.path.as_str()))
                .collect::<Vec<_>>(),
            [
                (ChangeStatus::Added, "docs/adr/0001-test.md"),
                (ChangeStatus::Modified, "docs/adr/0001-test.md"),
            ]
        );
    }

    #[test]
    fn ci_changed_entries_fails_closed_without_base_in_ci() {
        let error = ci_changed_entries(None, None, None, true)
            .expect_err("ci without a base ref should fail");

        assert!(error.to_string().contains("CRIV_BASE_REF"));
    }

    #[test]
    fn ci_changed_entries_reports_why_github_base_ref_is_unusable() {
        let root = two_commit_repository();
        let repository = GitRepository::open(root.path()).unwrap();
        let error = ci_changed_entries(Some(&repository), None, Some("missing".into()), true)
            .unwrap_err()
            .to_string();

        assert!(error.contains("origin/missing"), "{error}");
        assert!(error.contains("git diff missing HEAD failed"), "{error}");
    }

    #[test]
    fn ci_changed_entries_uses_explicit_base_ref() {
        let root = two_commit_repository();
        let base = git(root.path(), &["rev-parse", "HEAD~1"]);

        let repository = GitRepository::open(root.path()).unwrap();
        let selection = ci_changed_entries(Some(&repository), Some(&base), None, true).unwrap();
        let changes = selection.changes.expect("changes from explicit base ref");

        assert_eq!(selection.ci_base_ref.as_deref(), Some(base.as_str()));
        assert_eq!(changes.old_ref.as_deref(), Some(base.as_str()));
        assert_eq!(
            changes.new,
            Some(crate::git::Revision::Commit("HEAD".into()))
        );
        assert_eq!(changes.entries[0].status, ChangeStatus::Modified);
        assert_eq!(changes.entries[0].path, "tracked.txt");
    }

    #[test]
    fn ci_changed_entries_reports_the_failed_explicit_comparison() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        let repository = GitRepository::open(root.path()).unwrap();
        let error =
            ci_changed_entries(Some(&repository), Some("missing-base"), None, true).unwrap_err();

        assert!(error.to_string().contains("missing-base"));
        assert!(error.to_string().contains("git diff"));
    }

    #[test]
    fn manual_push_falls_back_to_the_last_commit_with_a_reported_basis() {
        let root = two_commit_repository();

        let changes = manual_push_changed_set(&GitRepository::open(root.path()).unwrap()).unwrap();

        assert_eq!(changes.entries[0].path, "tracked.txt");
        assert!(changes.basis.contains("HEAD~1..HEAD"));
        assert!(changes.basis.contains("@{upstream}"), "{}", changes.basis);
    }
}

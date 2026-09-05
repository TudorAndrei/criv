use std::collections::BTreeSet;
use std::env;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use usage::{Args as UsageArgs, ValueEnum};

use crate::check;
use crate::config::Config;
use crate::diagnostic;
use crate::git::{
    self, ChangeStatus, ChangedEntry, ChangedSet, ChangedSetComparison, GitRepository,
};
use crate::policy_scan::PolicyScanPlan;
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Stage {
    Commit,
    Push,
    Ci,
}

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
    #[usage(long, hide, requires = "pre_push")]
    remote_name: Option<String>,
    #[usage(long, hide, requires = "pre_push")]
    remote_url: Option<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "enforcement assembles one stage report from related vault and policy checks"
)]
pub fn run(root: &Path, options: &EnforceOptions) -> Result<()> {
    if options.pre_push && options.stage != Stage::Push {
        return Err(CrivError::usage(
            "--pre-push is only valid with --stage push",
        ));
    }
    crate::repository::RepositoryFiles::open_vault(root)?;
    let vault = Vault::load(root)?;
    let policy_plan = PolicyScanPlan::new(&vault);
    if !vault
        .config
        .enforce_stages
        .iter()
        .any(|stage| stage == options.stage.as_str())
    {
        return Err(CrivError::new(format!(
            "stage `{}` is not enabled in criv.toml",
            options.stage.as_str()
        )));
    }
    if options.stage == Stage::Ci
        && let Some(base_ref) = env_string("CRIV_BASE_REF")
    {
        crate::adr::check_base(root, &base_ref)?;
    }

    let diagnostics = check::validate_vault(&vault, None, &policy_plan);
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();

    let changed_entries = changed_entries(root, options)?;
    let changed_files = if options.stage == Stage::Ci {
        None
    } else {
        changed_entries.as_ref().map(ChangedSet::affected_paths)
    };
    let changed_policy_files = changed_files
        .as_ref()
        .map(|paths| paths.iter().cloned().collect::<BTreeSet<_>>());
    let violations = policy_plan
        .scan(&vault, changed_policy_files.as_ref())?
        .into_iter()
        .map(|violation| {
            format!(
                "{}:{}: {} policy `{}` matched `{}`",
                violation.path,
                violation.line,
                violation.adr_id,
                violation.pattern_id,
                violation.text
            )
        })
        .collect::<Vec<_>>();
    let import_violations = import_policy_violations(&vault, changed_files.as_ref());
    let receipt_is_current = crate::adr::receipt_is_current(root);
    let receipt_allows_transaction = options.stage == Stage::Commit
        && changed_entries
            .as_ref()
            .is_some_and(|changes| crate::adr::receipt_allows_transaction(root, &changes.entries));
    let source_receipt_is_current = crate::adr::source_receipt_is_current(root);
    let source_receipt_allows_transaction = options.stage == Stage::Commit
        && changed_entries.as_ref().is_some_and(|changes| {
            crate::adr::source_receipt_allows_transaction(root, &changes.entries)
        });
    let mut adr_violations = adr_immutability_violations(
        &vault.config.docs_dir,
        &vault.config.adr_dir,
        changed_entries
            .as_ref()
            .map(|changes| changes.entries.as_slice()),
        |entry| {
            (receipt_allows_transaction
                || source_receipt_allows_transaction
                || is_allowed_adr_change(root, changed_entries.as_ref(), entry))
                || (options.stage == Stage::Ci && is_branch_local_ci_change(root, entry))
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
    adr_violations.extend(config_scope_violations(
        root,
        changed_entries.as_ref(),
        &vault.config,
    ));
    let changed_count = changed_files.as_ref().map_or(0, Vec::len);
    let basis = changed_entries
        .as_ref()
        .map_or("no comparison", |changes| changes.basis.as_str());
    let failure = enforce_failure(violations, import_violations, adr_violations, errors);

    if options.format == Format::Json {
        let report = EnforceReport {
            stage: options.stage.as_str(),
            ok: failure.is_none(),
            errors,
            warnings,
            changed_files: changed_count,
            basis,
            violations: failure
                .as_ref()
                .map_or(&[][..], |failure| &failure.violations),
            code: failure.as_ref().map(|failure| failure.code),
            fix: failure.as_ref().map(|failure| failure.fix),
        };
        let json = serde_json::to_string_pretty(&report).map_err(|err| {
            CrivError::new(format!("failed to serialize enforcement report: {err}"))
        })?;
        println!("{json}");
        return match failure {
            Some(failure) => Err(CrivError::coded_fix(
                failure.code,
                failure.message,
                failure.fix,
            )),
            None => Ok(()),
        };
    }

    match options.stage {
        Stage::Commit => {
            println!(
                "commit enforcement: {errors} validation errors, {warnings} warnings, {changed_count} staged files ({basis})"
            );
        }
        Stage::Push => {
            println!(
                "push enforcement: {errors} validation errors, {warnings} warnings, {changed_count} changed files ({basis})"
            );
        }
        Stage::Ci => {
            println!("ci enforcement: {errors} validation errors, {warnings} warnings ({basis})");
        }
    }

    if let Some(failure) = failure {
        for violation in &failure.violations {
            println!("{violation}");
        }
        return Err(CrivError::coded_fix(
            failure.code,
            failure.message,
            failure.fix,
        ));
    }

    println!("enforcement passed");
    Ok(())
}

struct EnforceFailure {
    code: &'static str,
    message: String,
    fix: &'static str,
    violations: Vec<String>,
}

#[derive(Serialize)]
struct EnforceReport<'a> {
    stage: &'a str,
    ok: bool,
    errors: usize,
    warnings: usize,
    changed_files: usize,
    basis: &'a str,
    violations: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<&'static str>,
}

fn enforce_failure(
    violations: Vec<String>,
    import_violations: Vec<String>,
    adr_violations: Vec<String>,
    errors: usize,
) -> Option<EnforceFailure> {
    let (code, message, violations) = if !violations.is_empty() {
        (
            "policy-violation",
            format!("{} policy violation(s) found", violations.len()),
            violations,
        )
    } else if !import_violations.is_empty() {
        (
            "import-policy-violation",
            format!(
                "{} import policy violation(s) found",
                import_violations.len()
            ),
            import_violations,
        )
    } else if !adr_violations.is_empty() {
        (
            "adr-immutability-violation",
            format!(
                "{} ADR immutability violation(s) found",
                adr_violations.len()
            ),
            adr_violations,
        )
    } else if errors > 0 {
        (
            "enforcement-failed",
            "enforcement failed".into(),
            Vec::new(),
        )
    } else {
        return None;
    };
    Some(EnforceFailure {
        code,
        message,
        fix: diagnostic::fix_for(code).unwrap_or("Run `criv check` and repair the reported issue."),
        violations,
    })
}

fn import_policy_violations(vault: &Vault, changed_files: Option<&Vec<String>>) -> Vec<String> {
    let mut violations = Vec::new();
    for policy in &vault.config.import_policies {
        for file in vault.source_graph().files.values() {
            if changed_files.is_some_and(|files| !files.contains(&file.path)) {
                continue;
            }
            if !policy.scope_matcher.is_match(&file.path) {
                continue;
            }
            for import in &file.imports {
                if policy
                    .deny
                    .iter()
                    .zip(&policy.deny_matchers)
                    .any(|(pattern, matcher)| {
                        import_matches(pattern, matcher.as_ref(), &import.module)
                    })
                {
                    violations.push(format!(
                        "{}:{}: import policy `{}` denies `{}`",
                        file.path, import.line, policy.id, import.module
                    ));
                }
            }
        }
    }
    violations.sort();
    violations.dedup();
    violations
}

fn changed_entries(root: &Path, options: &EnforceOptions) -> Result<Option<ChangedSet>> {
    let repository = GitRepository::discover(root)?;
    match options.stage {
        Stage::Commit => repository
            .as_ref()
            .map(|repo| repo.changed_set(&ChangedSetComparison::Staged))
            .transpose(),
        Stage::Push if options.pre_push => pre_push_changed_entries(
            required_repository(repository.as_ref())?,
            options
                .remote_name
                .as_deref()
                .ok_or_else(|| CrivError::new("pre-push enforcement requires a remote name"))?,
            read_pre_push_updates()?,
        )
        .map(Some),
        // Manual invocations retain the documented best-effort upstream/last
        // commit fallback. Generated hooks always use the complete stdin mode.
        Stage::Push => repository
            .as_ref()
            .map(|repo| {
                repo.changed_set(&ChangedSetComparison::ThreeDot {
                    upstream_ref: "@{upstream}",
                    head_ref: "HEAD",
                })
                .or_else(|_| {
                    repo.changed_set(&ChangedSetComparison::Trees {
                        old_ref: "HEAD~1",
                        new_ref: "HEAD",
                    })
                })
            })
            .transpose(),
        Stage::Ci => ci_changed_entries(repository.as_ref()),
    }
}

fn ci_changed_entries(repository: Option<&GitRepository>) -> Result<Option<ChangedSet>> {
    ci_changed_entries_with_env(repository, env_string, is_ci_environment())
}

fn ci_changed_entries_with_env(
    repository: Option<&GitRepository>,
    env_value: impl Fn(&str) -> Option<String>,
    ci_environment: bool,
) -> Result<Option<ChangedSet>> {
    if let Some(base_ref) = env_value("CRIV_BASE_REF") {
        return required_repository(repository)?
            .changed_set(&ChangedSetComparison::Trees {
                old_ref: &base_ref,
                new_ref: "HEAD",
            })
            .map(Some);
    }

    if let Some(base_ref) = env_value("GITHUB_BASE_REF") {
        let origin_ref = format!("origin/{base_ref}");
        if let Some(repository) = repository
            && let Ok(changes) = repository.changed_set(&ChangedSetComparison::Trees {
                old_ref: &origin_ref,
                new_ref: "HEAD",
            })
        {
            return Ok(Some(changes));
        }
        if let Some(repository) = repository
            && let Ok(changes) = repository.changed_set(&ChangedSetComparison::Trees {
                old_ref: &base_ref,
                new_ref: "HEAD",
            })
        {
            return Ok(Some(changes));
        }
    }

    if ci_environment {
        return Err(CrivError::new(
            "ci enforcement requires CRIV_BASE_REF or a fetchable GITHUB_BASE_REF",
        ));
    }

    repository
        .map(|repo| repo.changed_set(&ChangedSetComparison::TreeToWorktree { old_ref: "HEAD" }))
        .transpose()
}

fn required_repository(repository: Option<&GitRepository>) -> Result<&GitRepository> {
    repository.ok_or_else(|| CrivError::new("git diff failed: not inside a Git worktree"))
}

fn env_string(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn is_ci_environment() -> bool {
    env::var("CI").is_ok_and(|value| value == "true")
        || env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PrePushUpdate {
    local_ref: String,
    local_oid: String,
    remote_ref: String,
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
            let [local_ref, local_oid, remote_ref, remote_oid] = fields.as_slice() else {
                return Err(CrivError::new(format!(
                    "invalid pre-push ref update `{line}`; expected local-ref local-oid remote-ref remote-oid"
                )));
            };
            for oid in [*local_oid, *remote_oid] {
                if oid.len() != 40 || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(CrivError::new(format!(
                        "invalid pre-push object ID `{oid}`"
                    )));
                }
            }
            Ok(PrePushUpdate {
                local_ref: (*local_ref).to_string(),
                local_oid: (*local_oid).to_string(),
                remote_ref: (*remote_ref).to_string(),
                remote_oid: (*remote_oid).to_string(),
            })
        })
        .collect()
}

fn pre_push_changed_entries(
    repository: &GitRepository,
    remote_name: &str,
    updates: Vec<PrePushUpdate>,
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
        new_ref: None,
        basis: format!("pre-push ref updates for remote {remote_name}"),
    })
}

fn is_zero_oid(oid: &str) -> bool {
    oid.bytes().all(|byte| byte == b'0')
}

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

fn is_allowed_adr_change(root: &Path, changes: Option<&ChangedSet>, entry: &ChangedEntry) -> bool {
    if changes
        .is_some_and(|changes| crate::adr::source_history_change_is_allowed(root, changes, entry))
    {
        return true;
    }
    // A committed receipt proves the entire tree transition, including paths
    // that Git reports as modifications when ADR mappings overlap.
    if entry
        .new_ref
        .as_deref()
        .is_some_and(|commit| crate::adr::receipt_allows_commit(root, commit))
    {
        return true;
    }
    if changes
        .is_some_and(|changes| crate::adr::receipt_allows_history_change(root, changes, entry))
    {
        return true;
    }
    if matches!(entry.status, ChangeStatus::Renamed | ChangeStatus::Deleted)
        && crate::adr::receipt_allows_change(root, entry)
    {
        return true;
    }
    if entry.status != ChangeStatus::Modified {
        return false;
    }
    let Some(old) = read_changed_content(root, entry.old_ref.as_deref(), &entry.path) else {
        return false;
    };
    let Some(new) = read_changed_content(root, entry.new_ref.as_deref(), &entry.path) else {
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

fn read_changed_content(root: &Path, git_ref: Option<&str>, path: &str) -> Option<String> {
    let Some(git_ref) = git_ref else {
        return crate::repository::RepositoryFiles::open(root)
            .ok()?
            .read_string(Path::new(path))
            .ok();
    };
    git::blob(root, git_ref, path).ok()
}

fn is_mechanical_wikilink_portability_migration(old: &str, new: &str) -> bool {
    old != new && normalize_portable_adr_links(new) == old
}

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

fn looks_like_decision(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "md")
        && path.split('/').any(|component| component == "adr")
        && !path.ends_with("/README.md")
}

fn is_adr_file(docs_dir: &str, adr_dir: &str, path: &str) -> bool {
    let adr_prefix = format!("{docs_dir}/{adr_dir}/");
    path.starts_with(&adr_prefix)
        && path != format!("{adr_prefix}README.md")
        && Path::new(path)
            .extension()
            .is_some_and(|extension| extension == "md")
}

fn import_matches(pattern: &str, matcher: Option<&crate::glob::GlobMatcher>, module: &str) -> bool {
    if let Some(matcher) = matcher {
        let normalized = module.replace("::", "/");
        return matcher.is_match(&normalized);
    }
    module == pattern || module.starts_with(&format!("{pattern}::"))
}

impl Stage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Push => "push",
            Self::Ci => "ci",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn only_a_decision_file_looks_like_a_decision() {
        assert!(looks_like_decision("docs/adr/0001-example.md"));
        assert!(looks_like_decision("documentation/adr/0002-other.md"));
        assert!(!looks_like_decision("docs/adr/README.md"));
        assert!(!looks_like_decision("docs/guide.md"));
        assert!(!looks_like_decision("src/adr.rs"));
    }

    #[test]
    fn import_patterns_match_exact_prefix_and_glob_forms() {
        assert!(import_matches("crate::infra", None, "crate::infra::db"));
        let glob = crate::glob::GlobMatcher::new(&["crate/infra/*".into()]).unwrap();
        assert!(import_matches(
            "crate::infra::*",
            Some(&glob),
            "crate::infra::db"
        ));
        assert!(import_matches("sqlx", None, "sqlx"));
        assert!(!import_matches(
            "crate::infra",
            None,
            "crate::infrastructure"
        ));
    }

    #[test]
    fn parses_multiple_pre_push_updates_and_rejects_malformed_records() {
        let updates = parse_pre_push_updates(
            "refs/heads/main 1111111111111111111111111111111111111111 refs/heads/main 0000000000000000000000000000000000000000\nrefs/heads/topic 2222222222222222222222222222222222222222 refs/heads/topic 3333333333333333333333333333333333333333\n",
        )
        .unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[1].remote_ref, "refs/heads/topic");
        assert!(parse_pre_push_updates("broken\n").is_err());
    }

    #[test]
    fn pre_push_checks_each_outgoing_commit_on_a_new_branch() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        git(root.path(), &["config", "user.email", "criv@example.com"]);
        git(root.path(), &["config", "user.name", "criv"]);
        std::fs::create_dir_all(root.path().join("docs/adr")).unwrap();
        std::fs::write(root.path().join("docs/adr/0001-test.md"), "first\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "add adr"]);
        std::fs::write(root.path().join("docs/adr/0001-test.md"), "changed\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "modify adr"]);
        let head = git_stdout(root.path(), &["rev-parse", "HEAD"]);

        let repository = GitRepository::discover(root.path()).unwrap();
        let changes = pre_push_changed_entries(
            repository.as_ref().unwrap(),
            "origin",
            vec![PrePushUpdate {
                local_ref: "refs/heads/main".into(),
                local_oid: head,
                remote_ref: "refs/heads/main".into(),
                remote_oid: "0000000000000000000000000000000000000000".into(),
            }],
        )
        .unwrap();
        let violations =
            adr_immutability_violations("docs", "adr", Some(&changes.entries), |entry| {
                is_allowed_adr_change(root.path(), Some(&changes), entry)
            });

        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("0001-test.md"));
    }

    #[test]
    fn ci_changed_entries_fails_closed_without_base_in_ci() {
        let error = ci_changed_entries_with_env(None, |_| None, true)
            .expect_err("ci without a base ref should fail");

        assert!(error.to_string().contains("CRIV_BASE_REF"));
    }

    #[test]
    fn ci_changed_entries_uses_explicit_base_ref() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        git(root.path(), &["config", "user.email", "criv@example.com"]);
        git(root.path(), &["config", "user.name", "criv"]);
        std::fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-m", "initial"]);
        let base = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        std::fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-m", "change"]);

        let repository = GitRepository::discover(root.path()).unwrap();
        let changes = ci_changed_entries_with_env(
            repository.as_ref(),
            |name| (name == "CRIV_BASE_REF").then(|| base.clone()),
            true,
        )
        .unwrap()
        .expect("changes from explicit base ref");

        assert_eq!(changes.old_ref.as_deref(), Some(base.as_str()));
        assert_eq!(changes.new_ref.as_deref(), Some("HEAD"));
        assert_eq!(changes.entries[0].status, ChangeStatus::Modified);
        assert_eq!(changes.entries[0].path, "tracked.txt");
    }

    #[test]
    fn ci_changed_entries_reports_the_failed_explicit_comparison() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        let repository = GitRepository::discover(root.path()).unwrap();
        let error = ci_changed_entries_with_env(
            repository.as_ref(),
            |name| (name == "CRIV_BASE_REF").then(|| "missing-base".into()),
            true,
        )
        .unwrap_err();

        assert!(error.to_string().contains("missing-base"));
        assert!(error.to_string().contains("git diff"));
    }

    #[test]
    fn manual_push_falls_back_to_the_last_commit_with_a_reported_basis() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        git(root.path(), &["config", "user.email", "criv@example.com"]);
        git(root.path(), &["config", "user.name", "criv"]);
        std::fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-m", "initial"]);
        std::fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-m", "change"]);

        let options = EnforceOptions {
            stage: Stage::Push,
            format: Format::Text,
            pre_push: false,
            remote_name: None,
            remote_url: None,
        };
        let changes = changed_entries(root.path(), &options).unwrap().unwrap();

        assert_eq!(changes.entries[0].path, "tracked.txt");
        assert!(changes.basis.contains("HEAD~1..HEAD"));
    }

    #[test]
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

        let violations = adr_immutability_violations("docs", "adr", Some(&entries), |_| false);

        assert_eq!(violations.len(), 2);
        assert!(violations[0].contains("0002-existing"));
        assert!(violations[1].contains("0003-existing"));
    }

    #[test]
    fn working_tree_migration_requires_regular_readable_content() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        git(root.path(), &["config", "user.email", "criv@example.com"]);
        git(root.path(), &["config", "user.name", "criv"]);
        let path = "docs/adr/0001-test.md";
        std::fs::create_dir_all(root.path().join("docs/adr")).unwrap();
        std::fs::write(root.path().join(path), "See [[ADR-0010]].\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "add adr"]);
        let entry = ChangedEntry {
            path: path.into(),
            previous_path: None,
            status: ChangeStatus::Modified,
            old_ref: Some("HEAD".into()),
            new_ref: None,
        };
        let migrated = "See [[0010-example|ADR-0010]].\n";
        std::fs::write(root.path().join(path), migrated).unwrap();
        assert!(is_allowed_adr_change(root.path(), None, &entry));
        std::fs::remove_file(root.path().join(path)).unwrap();
        assert!(!is_allowed_adr_change(root.path(), None, &entry));
        #[cfg(unix)]
        {
            let external = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(external.path(), migrated).unwrap();
            std::os::unix::fs::symlink(external.path(), root.path().join(path)).unwrap();
            assert!(!is_allowed_adr_change(root.path(), None, &entry));
        }
    }

    #[test]
    fn mechanical_adr_link_migrations_are_allowed() {
        let old = "See [[ADR-0010]] and [[ADR-0001#Context]].\n";
        let new = "See [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]] and [[docs/adr/0001-local-cli-vault-architecture#Context|ADR-0001#Context]].\n";

        assert!(is_mechanical_wikilink_portability_migration(old, new));
    }

    #[test]
    fn mechanical_adr_link_migrations_reject_content_edits() {
        let old = "See [[ADR-0010]].\n";
        let new = "Changed decision text and see [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]].\n";

        assert!(!is_mechanical_wikilink_portability_migration(old, new));
    }

    #[test]
    fn mechanical_adr_link_migrations_reject_mismatched_targets() {
        let old = "See [[ADR-0010]].\n";
        let new = "See [[0011-embed-runtime-skill-templates-as-assets|ADR-0010]].\n";

        assert!(!is_mechanical_wikilink_portability_migration(old, new));
    }

    #[test]
    fn adr_immutability_gate_can_allow_proven_link_migration() {
        let entries = vec![ChangedEntry {
            status: ChangeStatus::Modified,
            path: "docs/adr/0002-existing.md".into(),
            previous_path: None,
            old_ref: None,
            new_ref: None,
        }];

        let violations = adr_immutability_violations("docs", "adr", Some(&entries), |_| true);

        assert!(violations.is_empty());
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
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

    fn git_stdout(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
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
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }
}

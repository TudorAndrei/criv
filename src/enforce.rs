use std::collections::BTreeSet;
use std::env;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Output};

use clap::{Args as ClapArgs, ValueEnum};

use crate::check;
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Stage {
    Commit,
    Push,
    Ci,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct EnforceOptions {
    #[arg(long, value_enum)]
    stage: Stage,
    /// Consume Git's pre-push ref-update records from standard input.
    #[arg(long, hide = true)]
    pre_push: bool,
    #[arg(long, hide = true, requires = "pre_push")]
    remote_name: Option<String>,
    #[arg(long, hide = true, requires = "pre_push")]
    remote_url: Option<String>,
}

pub(crate) fn run(root: &Path, options: EnforceOptions) -> Result<()> {
    if options.pre_push && options.stage != Stage::Push {
        return Err(CrivError::usage(
            "--pre-push is only valid with --stage push",
        ));
    }
    let vault = Vault::load(root)?;
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

    let diagnostics = check::validate(&vault);
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();

    let changed_entries = changed_entries(root, &options)?;
    let changed_files = if options.stage == Stage::Ci {
        None
    } else {
        changed_entries
            .as_ref()
            .map(|changes| changed_entry_paths(&changes.entries))
    };
    let violations = policy_violations(root, &vault, changed_files.as_ref())?;
    let import_violations = import_policy_violations(&vault, changed_files.as_ref());
    let adr_violations = adr_immutability_violations(
        &vault.config.docs_dir,
        &vault.config.adr_dir,
        changed_entries
            .as_ref()
            .map(|changes| changes.entries.as_slice()),
        |entry| is_allowed_adr_link_migration(root, changed_entries.as_ref(), entry),
    );
    let tool_files = enforcement_files(&vault, changed_files.as_ref());
    let tool_errors = run_native_tools(root, &tool_files)?;

    match options.stage {
        Stage::Commit => {
            println!(
                "commit enforcement: {errors} validation errors, {warnings} warnings, {} staged files ({})",
                changed_files.as_ref().map_or(0, Vec::len),
                changed_entries
                    .as_ref()
                    .map_or("no comparison", |changes| &changes.basis)
            );
        }
        Stage::Push => {
            println!(
                "push enforcement: {errors} validation errors, {warnings} warnings, {} changed files ({})",
                changed_files.as_ref().map_or(0, Vec::len),
                changed_entries
                    .as_ref()
                    .map_or("no comparison", |changes| &changes.basis)
            );
        }
        Stage::Ci => {
            println!(
                "ci enforcement: {errors} validation errors, {warnings} warnings ({})",
                changed_entries
                    .as_ref()
                    .map_or("no comparison", |changes| &changes.basis)
            );
        }
    }

    if !violations.is_empty() {
        for violation in &violations {
            println!("{violation}");
        }
        return Err(CrivError::new(format!(
            "{} policy violation(s) found",
            violations.len()
        )));
    }
    if !import_violations.is_empty() {
        for violation in &import_violations {
            println!("{violation}");
        }
        return Err(CrivError::new(format!(
            "{} import policy violation(s) found",
            import_violations.len()
        )));
    }
    if !adr_violations.is_empty() {
        for violation in &adr_violations {
            println!("{violation}");
        }
        return Err(CrivError::new(format!(
            "{} ADR immutability violation(s) found",
            adr_violations.len()
        )));
    }

    if errors > 0 {
        return Err(CrivError::new("enforcement failed"));
    }
    if tool_errors > 0 {
        return Err(CrivError::new(format!(
            "{} native enforcement tool(s) failed",
            tool_errors
        )));
    }

    println!("enforcement passed");
    Ok(())
}

fn policy_violations(
    root: &Path,
    vault: &Vault,
    changed_files: Option<&Vec<String>>,
) -> Result<Vec<String>> {
    struct ScanRecord<'a> {
        adr_id: String,
        pattern_id: String,
        scopes: BTreeSet<String>,
        pattern: &'a crate::vault::PolicyPattern,
    }

    let mut records = Vec::new();
    for note in &vault.notes {
        if note.status.as_deref() != Some("accepted") {
            continue;
        }
        let Some(adr_id) = &note.id else {
            continue;
        };
        let scopes: BTreeSet<String> =
            policy_scan_files(vault, &vault.effective_governs(note), changed_files)
                .into_iter()
                .collect();
        for pattern in &note.policy_patterns {
            if !crate::structural::policy_pattern_entry_is_valid(pattern) {
                continue;
            }
            let Some(local_id) = pattern
                .id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            records.push(ScanRecord {
                adr_id: adr_id.clone(),
                pattern_id: format!("{adr_id}/{local_id}"),
                scopes: scopes.clone(),
                pattern,
            });
        }
    }

    let requests = records
        .iter()
        .enumerate()
        .map(|(key, record)| crate::structural::PolicyScanRequest {
            key,
            policy: record.pattern,
            paths: &record.scopes,
        })
        .collect::<Vec<_>>();
    let rows_by_key = crate::structural::find_policies_batch(root, vault, &requests)?;

    let mut violations = Vec::new();
    for (key, record) in records.iter().enumerate() {
        if let Some(rows) = rows_by_key.get(&key) {
            for row in rows {
                violations.push(format!(
                    "{}:{}: {} policy `{pattern_id}` matched `{}`",
                    row.path,
                    row.line,
                    record.adr_id,
                    row.text,
                    pattern_id = record.pattern_id
                ));
            }
        }
    }
    Ok(violations)
}

fn policy_scan_files(
    vault: &Vault,
    scopes: &[String],
    changed_files: Option<&Vec<String>>,
) -> Vec<String> {
    let files = policy_scope_files(vault, scopes);
    let Some(changed_files) = changed_files else {
        return files;
    };
    let changed = changed_files.iter().collect::<BTreeSet<_>>();
    files
        .into_iter()
        .filter(|file| changed.contains(file))
        .collect()
}

fn policy_scope_files(vault: &Vault, scopes: &[String]) -> Vec<String> {
    scopes
        .iter()
        .flat_map(|scope| vault.source_files_matching_glob(scope))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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

#[derive(Debug, Clone, Eq, PartialEq)]
struct ChangedSet {
    entries: Vec<ChangedEntry>,
    old_ref: Option<String>,
    new_ref: Option<String>,
    basis: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ChangedEntry {
    status: ChangeStatus,
    path: String,
    previous_path: Option<String>,
    old_ref: Option<String>,
    new_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Other,
}

fn changed_entries(root: &Path, options: &EnforceOptions) -> Result<Option<ChangedSet>> {
    match options.stage {
        Stage::Commit if !is_git_repository(root)? => Ok(None),
        Stage::Commit => git_changed_set(
            root,
            &["diff", "--name-status", "-z", "--cached"],
            Some("HEAD"),
            Some(":"),
        )
        .map(Some),
        Stage::Push if options.pre_push => pre_push_changed_entries(
            root,
            options
                .remote_name
                .as_deref()
                .ok_or_else(|| CrivError::new("pre-push enforcement requires a remote name"))?,
            read_pre_push_updates()?,
        )
        .map(Some),
        // Manual invocations retain the documented best-effort upstream/last
        // commit fallback. Generated hooks always use the complete stdin mode.
        Stage::Push if !is_git_repository(root)? => Ok(None),
        Stage::Push => git_changed_set(
            root,
            &["diff", "--name-status", "-z", "@{upstream}...HEAD"],
            Some("@{upstream}"),
            Some("HEAD"),
        )
        .or_else(|_| {
            git_changed_set(
                root,
                &["diff", "--name-status", "-z", "HEAD~1..HEAD"],
                Some("HEAD~1"),
                Some("HEAD"),
            )
        })
        .map(Some),
        Stage::Ci => ci_changed_entries(root),
    }
}

fn ci_changed_entries(root: &Path) -> Result<Option<ChangedSet>> {
    ci_changed_entries_with_env(root, env_string, is_ci_environment())
}

fn ci_changed_entries_with_env(
    root: &Path,
    env_value: impl Fn(&str) -> Option<String>,
    ci_environment: bool,
) -> Result<Option<ChangedSet>> {
    if let Some(base_ref) = env_value("CRIV_BASE_REF") {
        return git_changed_set(
            root,
            &["diff", "--name-status", "-z", &base_ref, "HEAD"],
            Some(&base_ref),
            Some("HEAD"),
        )
        .map(Some);
    }

    if let Some(base_ref) = env_value("GITHUB_BASE_REF") {
        let origin_ref = format!("origin/{base_ref}");
        if let Ok(changes) = git_changed_set(
            root,
            &["diff", "--name-status", "-z", &origin_ref, "HEAD"],
            Some(&origin_ref),
            Some("HEAD"),
        ) {
            return Ok(Some(changes));
        }
        if let Ok(changes) = git_changed_set(
            root,
            &["diff", "--name-status", "-z", &base_ref, "HEAD"],
            Some(&base_ref),
            Some("HEAD"),
        ) {
            return Ok(Some(changes));
        }
    }

    if ci_environment {
        return Err(CrivError::new(
            "ci enforcement requires CRIV_BASE_REF or a fetchable GITHUB_BASE_REF",
        ));
    }

    if !is_git_repository(root)? {
        return Ok(None);
    }
    git_changed_set(
        root,
        &["diff", "--name-status", "-z", "HEAD"],
        Some("HEAD"),
        None,
    )
    .map(Some)
}

fn env_string(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn is_ci_environment() -> bool {
    env::var("CI").is_ok_and(|value| value == "true")
        || env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
}

fn git_output(root: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|err| CrivError::new(format!("failed to run `git {}`: {err}", args.join(" "))))?;
    if output.status.success() {
        return Ok(output);
    }
    Err(CrivError::new(format!(
        "`git {}` failed with {}: {}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn is_git_repository(root: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|err| {
            CrivError::new(format!(
                "failed to run `git rev-parse --is-inside-work-tree`: {err}"
            ))
        })?;
    Ok(output.status.success() && output.stdout == b"true\n")
}

fn git_changed_set(
    root: &Path,
    args: &[&str],
    old_ref: Option<&str>,
    new_ref: Option<&str>,
) -> Result<ChangedSet> {
    let output = git_output(root, args)?;
    let mut entries = parse_changed_entries(&output.stdout)?;
    for entry in &mut entries {
        entry.old_ref = old_ref.map(str::to_string);
        entry.new_ref = new_ref.map(str::to_string);
    }
    Ok(ChangedSet {
        entries,
        old_ref: old_ref.map(str::to_string),
        new_ref: new_ref.map(str::to_string),
        basis: match (old_ref, new_ref) {
            (Some(old), Some(new)) => format!("Git comparison {old}..{new}"),
            (Some(old), None) => format!("Git comparison {old}..worktree"),
            (None, Some(new)) => format!("Git comparison ..{new}"),
            (None, None) => "Git comparison".into(),
        },
    })
}

fn parse_changed_entries(stdout: &[u8]) -> Result<Vec<ChangedEntry>> {
    let mut entries = Vec::new();
    let mut fields = stdout
        .split(|byte| *byte == b'\0')
        .filter(|field| !field.is_empty());
    while let Some(status_field) = fields.next() {
        let status = status_field.first().copied().ok_or_else(|| {
            CrivError::new("Git name-status output contained an empty status field")
        })? as char;
        let change_status = match status {
            'A' => ChangeStatus::Added,
            'M' | 'T' => ChangeStatus::Modified,
            'D' => ChangeStatus::Deleted,
            'R' => ChangeStatus::Renamed,
            'C' => ChangeStatus::Copied,
            _ => ChangeStatus::Other,
        };
        let (path, previous_path) = match change_status {
            ChangeStatus::Renamed | ChangeStatus::Copied => {
                let previous_path = next_git_path(&mut fields)?;
                let path = next_git_path(&mut fields)?;
                (path, Some(previous_path))
            }
            _ => (next_git_path(&mut fields)?, None),
        };
        entries.push(ChangedEntry {
            status: change_status,
            path,
            previous_path,
            old_ref: None,
            new_ref: None,
        });
    }
    Ok(entries)
}

fn next_git_path<'a>(fields: &mut impl Iterator<Item = &'a [u8]>) -> Result<String> {
    let value = fields
        .next()
        .ok_or_else(|| CrivError::new("Git name-status output ended before a path field"))?;
    String::from_utf8(value.to_vec()).map_err(|_| {
        CrivError::new("Git changed path is not valid UTF-8; criv cannot represent it")
    })
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
            if fields.len() != 4 {
                return Err(CrivError::new(format!(
                    "invalid pre-push ref update `{line}`; expected local-ref local-oid remote-ref remote-oid"
                )));
            }
            for oid in [fields[1], fields[3]] {
                if oid.len() != 40 || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(CrivError::new(format!(
                        "invalid pre-push object ID `{oid}`"
                    )));
                }
            }
            Ok(PrePushUpdate {
                local_ref: fields[0].to_string(),
                local_oid: fields[1].to_string(),
                remote_ref: fields[2].to_string(),
                remote_oid: fields[3].to_string(),
            })
        })
        .collect()
}

fn pre_push_changed_entries(
    root: &Path,
    remote_name: &str,
    updates: Vec<PrePushUpdate>,
) -> Result<ChangedSet> {
    let mut entries = Vec::new();
    for update in updates {
        if is_zero_oid(&update.local_oid) {
            continue;
        }
        for commit in outgoing_commits(root, remote_name, &update)? {
            let output = git_output(
                root,
                &[
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "--name-status",
                    "-z",
                    "-r",
                    &commit,
                ],
            )?;
            let old_ref = git_first_parent(root, &commit)?;
            for mut entry in parse_changed_entries(&output.stdout)? {
                entry.old_ref = old_ref.clone();
                entry.new_ref = Some(commit.clone());
                entries.push(entry);
            }
        }
    }
    Ok(ChangedSet {
        entries,
        old_ref: None,
        new_ref: None,
        basis: format!("pre-push ref updates for remote {remote_name}"),
    })
}

fn outgoing_commits(root: &Path, remote_name: &str, update: &PrePushUpdate) -> Result<Vec<String>> {
    let range = if is_zero_oid(&update.remote_oid) {
        None
    } else {
        Some(format!("{}..{}", update.remote_oid, update.local_oid))
    };
    let remote_arg = format!("--remotes={remote_name}");
    let args = match &range {
        Some(range) => vec!["rev-list", "--reverse", range],
        None => vec![
            "rev-list",
            "--reverse",
            &update.local_oid,
            "--not",
            &remote_arg,
        ],
    };
    let output = git_output(root, &args)?;
    String::from_utf8(output.stdout)
        .map_err(|_| CrivError::new("Git rev-list output is not valid UTF-8"))
        .map(|stdout| stdout.lines().map(str::to_string).collect())
}

fn git_first_parent(root: &Path, commit: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", &format!("{commit}^")])
        .output()
        .map_err(|err| {
            CrivError::new(format!("failed to run git rev-parse for {commit}: {err}"))
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    String::from_utf8(output.stdout)
        .map_err(|_| CrivError::new("Git parent object ID is not valid UTF-8"))
        .map(|stdout| Some(stdout.trim().to_string()))
}

fn is_zero_oid(oid: &str) -> bool {
    oid.bytes().all(|byte| byte == b'0')
}

fn changed_entry_paths(entries: &[ChangedEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| entry.status != ChangeStatus::Deleted)
        .map(|entry| entry.path.clone())
        .collect()
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
        if entry.status == ChangeStatus::Modified && is_allowed_change(entry) {
            continue;
        }

        let display_path = entry
            .previous_path
            .as_ref()
            .map(|previous| format!("{previous} -> {}", entry.path))
            .unwrap_or_else(|| entry.path.clone());
        violations.push(format!(
            "{display_path}: ADR files are immutable; add a new ADR with `supersedes` instead of modifying an existing one"
        ));
    }
    violations
}

fn is_allowed_adr_link_migration(
    root: &Path,
    _changes: Option<&ChangedSet>,
    entry: &ChangedEntry,
) -> bool {
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

fn read_changed_content(root: &Path, git_ref: Option<&str>, path: &str) -> Option<String> {
    let Some(git_ref) = git_ref else {
        return std::fs::read_to_string(root.join(path)).ok();
    };
    let object = if git_ref == ":" {
        format!(":{path}")
    } else {
        format!("{git_ref}:{path}")
    };
    let output = Command::new("git")
        .current_dir(root)
        .args(["show", &object])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn is_mechanical_wikilink_portability_migration(old: &str, new: &str) -> bool {
    old != new && normalize_portable_adr_links(new) == old
}

fn normalize_portable_adr_links(markdown: &str) -> String {
    let mut normalized = String::with_capacity(markdown.len());
    let mut start = 0;
    while let Some(open) = markdown[start..].find("[[") {
        let open = start + open;
        let body_start = open + 2;
        let Some(close_offset) = markdown[body_start..].find("]]") else {
            break;
        };
        let close = body_start + close_offset;
        normalized.push_str(&markdown[start..open]);
        let body = &markdown[body_start..close];
        if let Some(alias) = portable_adr_link_alias(body) {
            normalized.push_str("[[");
            normalized.push_str(alias);
            normalized.push_str("]]");
        } else {
            normalized.push_str(&markdown[open..close + 2]);
        }
        start = close + 2;
    }
    normalized.push_str(&markdown[start..]);
    normalized
}

fn portable_adr_link_alias(body: &str) -> Option<&str> {
    let (target, alias) = body.split_once('|')?;
    let alias = alias.trim();
    let alias_base = alias.split('#').next().unwrap_or(alias);
    if !crate::util::is_adr_id(alias_base) {
        return None;
    }
    let target_fragment = target.split_once('#').map(|(_, fragment)| fragment);
    let alias_fragment = alias.split_once('#').map(|(_, fragment)| fragment);
    if target_fragment != alias_fragment {
        return None;
    }
    let number = &alias_base[4..];
    let target_base = target.split('#').next().unwrap_or(target).trim();
    let basename = target_base
        .trim_end_matches(".md")
        .split('/')
        .next_back()
        .unwrap_or(target_base);
    (basename == number || basename.starts_with(&format!("{number}-"))).then_some(alias)
}

fn is_adr_file(docs_dir: &str, adr_dir: &str, path: &str) -> bool {
    let adr_prefix = format!("{docs_dir}/{adr_dir}/");
    path.starts_with(&adr_prefix)
        && path != format!("{adr_prefix}README.md")
        && Path::new(path)
            .extension()
            .is_some_and(|extension| extension == "md")
}

fn enforcement_files(vault: &Vault, changed_files: Option<&Vec<String>>) -> Vec<String> {
    vault
        .source_files()
        .iter()
        .filter(|path| changed_files.is_none_or(|files| files.contains(path)))
        .cloned()
        .collect()
}

fn run_native_tools(root: &Path, files: &[String]) -> Result<usize> {
    let js_ts = files
        .iter()
        .filter(|path| matches_extension(path, &["js", "jsx", "ts", "tsx", "mjs", "cjs"]))
        .cloned()
        .collect::<Vec<_>>();
    let python = files
        .iter()
        .filter(|path| matches_extension(path, &["py"]))
        .cloned()
        .collect::<Vec<_>>();

    let mut failures = 0;
    failures += run_optional_tool(root, "Oxlint", tool_on_path("oxlint"), &js_ts)?;
    failures += run_optional_tool(root, "Ruff", tool_on_path("ruff"), &python)?;
    Ok(failures)
}

fn run_optional_tool(
    root: &Path,
    label: &str,
    command: ToolCommand,
    files: &[String],
) -> Result<usize> {
    if files.is_empty() {
        return Ok(0);
    }

    let mut process = Command::new(command.program());
    process.current_dir(root);
    if label == "Ruff" {
        process.arg("check");
    }
    process.arg("--");
    process.args(files);

    match process.output() {
        Ok(output) if output.status.success() => {
            println!("{label}: checked {} file(s)", files.len());
            Ok(0)
        }
        Ok(output) => {
            println!("{label}: failed");
            print_tool_output(&output.stdout);
            print_tool_output(&output.stderr);
            Ok(1)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("{label}: skipped {} file(s); tool not found", files.len());
            Ok(0)
        }
        Err(err) => Err(CrivError::new(format!("failed to run {label}: {err}"))),
    }
}

fn print_tool_output(bytes: &[u8]) {
    let output = String::from_utf8_lossy(bytes);
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        println!("{line}");
    }
}

fn tool_on_path(name: &'static str) -> ToolCommand {
    ToolCommand::Name(name)
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum ToolCommand {
    Name(&'static str),
}

impl ToolCommand {
    fn program(&self) -> &std::ffi::OsStr {
        match self {
            Self::Name(name) => std::ffi::OsStr::new(name),
        }
    }
}

fn matches_extension(path: &str, extensions: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extensions.contains(&extension))
}

fn import_matches(pattern: &str, matcher: Option<&crate::util::GlobMatcher>, module: &str) -> bool {
    if let Some(matcher) = matcher {
        let normalized = module.replace("::", "/");
        return matcher.is_match(&normalized);
    }
    module == pattern || module.starts_with(&format!("{pattern}::"))
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Commit => "commit",
            Stage::Push => "push",
            Stage::Ci => "ci",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_patterns_match_exact_prefix_and_glob_forms() {
        assert!(import_matches("crate::infra", None, "crate::infra::db"));
        let glob = crate::util::GlobMatcher::new(&["crate/infra/*".into()]).unwrap();
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
    fn parses_git_name_status_entries() {
        let entries = parse_changed_entries(
            b"A\0docs/adr/0012-new.md\0M\0src/enforce.rs\0R100\0docs/adr/0001-old.md\0docs/adr/0001-renamed.md\0",
        )
        .unwrap();

        assert_eq!(entries[0].status, ChangeStatus::Added);
        assert_eq!(entries[0].path, "docs/adr/0012-new.md");
        assert_eq!(entries[2].status, ChangeStatus::Renamed);
        assert_eq!(
            entries[2].previous_path.as_deref(),
            Some("docs/adr/0001-old.md")
        );
        assert_eq!(entries[2].path, "docs/adr/0001-renamed.md");
    }

    #[test]
    fn rejects_non_utf8_git_paths_in_name_status_output() {
        let error = parse_changed_entries(b"M\0docs/adr/\xff.md\0").unwrap_err();
        assert!(error.to_string().contains("not valid UTF-8"));
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

        let changes = pre_push_changed_entries(
            root.path(),
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
                is_allowed_adr_link_migration(root.path(), Some(&changes), entry)
            });

        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("0001-test.md"));
    }

    #[test]
    fn ci_changed_entries_fails_closed_without_base_in_ci() {
        let root = tempfile::TempDir::new().unwrap();
        let error = ci_changed_entries_with_env(root.path(), |_| None, true)
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

        let changes = ci_changed_entries_with_env(
            root.path(),
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
        let error = ci_changed_entries_with_env(
            root.path(),
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

    #[test]
    fn native_tool_commands_use_path_names_only() {
        let root = tempfile::TempDir::new().unwrap();
        let oxlint = root
            .path()
            .join(".obsidian/plugins/criv/node_modules/.bin/oxlint");
        let ruff = root.path().join(".venv/bin/ruff");
        std::fs::create_dir_all(oxlint.parent().unwrap()).unwrap();
        std::fs::create_dir_all(ruff.parent().unwrap()).unwrap();
        std::fs::write(&oxlint, "").unwrap();
        std::fs::write(&ruff, "").unwrap();

        assert_eq!(tool_on_path("oxlint"), ToolCommand::Name("oxlint"));
        assert_eq!(tool_on_path("ruff"), ToolCommand::Name("ruff"));
    }

    #[test]
    fn policy_scan_files_intersects_changed_files_with_governed_sources() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "fn run() {}\n").unwrap();
        std::fs::write(root.path().join("src/other.rs"), "fn other() {}\n").unwrap();
        let vault = Vault::load(root.path()).unwrap();

        let changed = vec!["src/lib.rs".into(), "docs/readme.md".into()];
        assert_eq!(
            policy_scan_files(&vault, &["src/**".into()], Some(&changed)),
            vec!["src/lib.rs"]
        );
        assert_eq!(
            policy_scan_files(&vault, &["src/other.rs#fn:other".into()], Some(&changed)),
            Vec::<String>::new()
        );
        assert_eq!(
            policy_scan_files(&vault, &["src/other.rs#fn:other".into()], None),
            vec!["src/other.rs"]
        );
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
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

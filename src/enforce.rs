use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

pub(crate) fn run(root: &Path, options: EnforceOptions) -> Result<()> {
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

    let changed_entries = changed_entries(root, options.stage);
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
                "commit enforcement: {errors} validation errors, {warnings} warnings, {} staged files",
                changed_files.as_ref().map_or(0, Vec::len)
            );
        }
        Stage::Push => {
            println!(
                "push enforcement: {errors} validation errors, {warnings} warnings, {} changed files",
                changed_files.as_ref().map_or(0, Vec::len)
            );
        }
        Stage::Ci => {
            println!("ci enforcement: {errors} validation errors, {warnings} warnings");
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
    let mut violations = Vec::new();
    for note in &vault.notes {
        if note.status.as_deref() != Some("accepted") {
            continue;
        }
        let Some(adr_id) = &note.id else {
            continue;
        };
        let scopes = vault.effective_governs(note);
        for pattern in &note.policy_pattern_ids {
            let pattern_id = format!("{adr_id}/{pattern}");
            let rows =
                crate::structural::find_policy_pattern(root, vault, &pattern_id, pattern, &scopes)?;
            for row in rows {
                if changed_files.is_some_and(|files| !files.contains(&row.path)) {
                    continue;
                }
                violations.push(format!(
                    "{}:{}: {} policy `{pattern_id}` matched `{}`",
                    row.path, row.line, adr_id, row.text
                ));
            }
        }
    }
    Ok(violations)
}

fn import_policy_violations(vault: &Vault, changed_files: Option<&Vec<String>>) -> Vec<String> {
    let mut violations = Vec::new();
    for policy in &vault.config.import_policies {
        for file in vault.source_graph().files.values() {
            if changed_files.is_some_and(|files| !files.contains(&file.path)) {
                continue;
            }
            if !policy
                .scope
                .iter()
                .any(|pattern| path_matches(pattern, &file.path))
            {
                continue;
            }
            for import in &file.imports {
                if policy
                    .deny
                    .iter()
                    .any(|pattern| import_matches(pattern, &import.module))
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
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ChangedEntry {
    status: ChangeStatus,
    path: String,
    previous_path: Option<String>,
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

fn changed_entries(root: &Path, stage: Stage) -> Option<ChangedSet> {
    match stage {
        Stage::Commit => git_changed_set(
            root,
            &["diff", "--name-status", "--cached"],
            Some("HEAD"),
            Some(":"),
        ),
        Stage::Push => git_changed_set(
            root,
            &["diff", "--name-status", "@{upstream}...HEAD"],
            Some("@{upstream}"),
            Some("HEAD"),
        )
        .or_else(|| {
            git_changed_set(
                root,
                &["diff", "--name-status", "HEAD~1..HEAD"],
                Some("HEAD~1"),
                Some("HEAD"),
            )
        }),
        Stage::Ci => ci_changed_entries(root),
    }
}

fn ci_changed_entries(root: &Path) -> Option<ChangedSet> {
    if let Ok(base_ref) = env::var("CRIV_BASE_REF")
        && let Some(changes) = git_changed_set(
            root,
            &["diff", "--name-status", &base_ref, "HEAD"],
            Some(&base_ref),
            Some("HEAD"),
        )
    {
        return Some(changes);
    }

    if let Ok(base_ref) = env::var("GITHUB_BASE_REF") {
        let origin_ref = format!("origin/{base_ref}");
        if let Some(changes) = git_changed_set(
            root,
            &["diff", "--name-status", &origin_ref, "HEAD"],
            Some(&origin_ref),
            Some("HEAD"),
        ) {
            return Some(changes);
        }
        if let Some(changes) = git_changed_set(
            root,
            &["diff", "--name-status", &base_ref, "HEAD"],
            Some(&base_ref),
            Some("HEAD"),
        ) {
            return Some(changes);
        }
    }

    git_changed_set(root, &["diff", "--name-status", "HEAD"], Some("HEAD"), None)
}

fn git_changed_entries(root: &Path, args: &[&str]) -> Option<Vec<ChangedEntry>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_changed_entries(&stdout)
}

fn git_changed_set(
    root: &Path,
    args: &[&str],
    old_ref: Option<&str>,
    new_ref: Option<&str>,
) -> Option<ChangedSet> {
    git_changed_entries(root, args).map(|entries| ChangedSet {
        entries,
        old_ref: old_ref.map(str::to_string),
        new_ref: new_ref.map(str::to_string),
    })
}

fn parse_changed_entries(stdout: &str) -> Option<Vec<ChangedEntry>> {
    let mut entries = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        let status = fields.first().and_then(|field| field.chars().next())?;
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
                let previous_path = fields.get(1)?.to_string();
                let path = fields.get(2)?.to_string();
                (path, Some(previous_path))
            }
            _ => (fields.get(1)?.to_string(), None),
        };
        entries.push(ChangedEntry {
            status: change_status,
            path,
            previous_path,
        });
    }
    Some(entries)
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
    changes: Option<&ChangedSet>,
    entry: &ChangedEntry,
) -> bool {
    if entry.status != ChangeStatus::Modified {
        return false;
    }
    let Some(changes) = changes else {
        return false;
    };
    let Some(old) = read_changed_content(root, changes.old_ref.as_deref(), &entry.path) else {
        return false;
    };
    let Some(new) = read_changed_content(root, changes.new_ref.as_deref(), &entry.path) else {
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
    failures += run_optional_tool(
        root,
        "ESLint",
        local_or_path(root, "node_modules/.bin/eslint", "eslint"),
        &js_ts,
    )?;
    failures += run_optional_tool(
        root,
        "Ruff",
        local_or_path(root, ".venv/bin/ruff", "ruff"),
        &python,
    )?;
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

fn local_or_path(root: &Path, local: &str, fallback: &'static str) -> ToolCommand {
    let local = root.join(local);
    if local.exists() {
        ToolCommand::Path(local)
    } else {
        ToolCommand::Name(fallback)
    }
}

enum ToolCommand {
    Path(PathBuf),
    Name(&'static str),
}

impl ToolCommand {
    fn program(&self) -> &std::ffi::OsStr {
        match self {
            Self::Path(path) => path.as_os_str(),
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

fn path_matches(pattern: &str, path: &str) -> bool {
    crate::util::glob_matches(pattern, path)
}

fn import_matches(pattern: &str, module: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        let normalized = module.replace("::", "/");
        return crate::util::glob_matches(&pattern.replace("::", "/"), &normalized);
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
        assert!(import_matches("crate::infra", "crate::infra::db"));
        assert!(import_matches("crate::infra::*", "crate::infra::db"));
        assert!(import_matches("sqlx", "sqlx"));
        assert!(!import_matches("crate::infra", "crate::infrastructure"));
    }

    #[test]
    fn parses_git_name_status_entries() {
        let entries = parse_changed_entries(
            "A\tdocs/adr/0012-new.md\nM\tsrc/enforce.rs\nR100\tdocs/adr/0001-old.md\tdocs/adr/0001-renamed.md\n",
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
    fn adr_immutability_allows_new_adrs_but_blocks_existing_changes() {
        let entries = vec![
            ChangedEntry {
                status: ChangeStatus::Added,
                path: "docs/adr/0012-new.md".into(),
                previous_path: None,
            },
            ChangedEntry {
                status: ChangeStatus::Modified,
                path: "docs/adr/0002-existing.md".into(),
                previous_path: None,
            },
            ChangedEntry {
                status: ChangeStatus::Renamed,
                path: "docs/adr/0003-renamed.md".into(),
                previous_path: Some("docs/adr/0003-existing.md".into()),
            },
            ChangedEntry {
                status: ChangeStatus::Modified,
                path: "docs/adr/README.md".into(),
                previous_path: None,
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
        }];

        let violations = adr_immutability_violations("docs", "adr", Some(&entries), |_| true);

        assert!(violations.is_empty());
    }
}

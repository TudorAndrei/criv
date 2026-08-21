use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use std::fs;

use clap::{Args as ClapArgs, ValueEnum};
use rumdl_lib::config::Config as RumdlConfig;
use rumdl_lib::fix_coordinator::FixCoordinator;
use rumdl_lib::rule::{LintWarning, Rule};
use rumdl_lib::rules::{all_rules, filter_rules};
use serde::Serialize;

use crate::diagnostic::{LspRange, SourceLocation};
use crate::discovery::{
    MarkdownPolicy, discover_markdown, read_selected_text_from, select_markdown,
};
#[cfg(test)]
use crate::git::ChangedEntry;
use crate::git::{ChangeStatus, ChangedSet};
use crate::policy_scan::{PolicyDiagnostic, PolicyDiagnosticKind, PolicyScanPlan};
use crate::repository::RepositoryFiles;
use crate::state::{self, State};
use crate::util::{is_adr_id, kebab};
use crate::vault::{
    Note, NoteKind, ResolvedLink, SourceTargetResolution, Vault, is_typed_source_target,
    source_target_body,
};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Github,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct CheckOptions {
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long)]
    fix: bool,
    /// Validate safely scoped facts for the staged Git transaction.
    #[arg(long, conflicts_with = "fix")]
    changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Diagnostic {
    severity: Severity,
    code: &'static str,
    path: String,
    line: Option<usize>,
    message: String,
    location: Option<SourceLocation>,
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    severity: Severity,
    code: &'static str,
    path: &'a str,
    line: Option<usize>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<LspRange>,
}

#[derive(Clone, Copy)]
struct MarkdownFixScope<'a> {
    files: &'a RepositoryFiles,
    docs_dir: &'a Path,
}

impl Diagnostic {
    pub(crate) fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }

    pub(crate) fn is_warning(&self) -> bool {
        matches!(self.severity, Severity::Warning)
    }

    pub(crate) fn describe(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}: {}", self.path, self.message),
            None => format!("{}: {}", self.path, self.message),
        }
    }

    fn json(&self) -> JsonDiagnostic<'_> {
        JsonDiagnostic {
            severity: self.severity,
            code: self.code,
            path: &self.path,
            line: self.line,
            message: &self.message,
            range: self.location.as_ref().map(SourceLocation::lsp_range),
        }
    }
}

pub(crate) fn run(root: &Path, options: CheckOptions) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    let mut diagnostics = if options.changed {
        validate_changed(&files)?
    } else {
        validate_all_with_fix(&files, options.fix)?
    };

    if let Some(filter) = &options.filter {
        diagnostics.retain(|diag| {
            diag.path.contains(filter)
                || diag.message.contains(filter)
                || diag.code.contains(filter)
        });
    }

    match options.format {
        Format::Text => {
            print_text(&diagnostics);
            let stale_skills = if options.changed {
                Vec::new()
            } else {
                crate::install::skill_inventory_from(&files).advisory_outdated_paths()
            };
            if !stale_skills.is_empty() {
                let subject = if stale_skills.len() == 1 {
                    "skill is"
                } else {
                    "skills are"
                };
                println!(
                    "note: {} agent {subject} out of date; run `criv init --force-skills`",
                    stale_skills.len()
                );
            }
        }
        Format::Json => print_json(&diagnostics)?,
        Format::Github => print_github(&diagnostics),
    }

    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(CrivError::new("check failed"));
    }

    Ok(())
}

pub(crate) fn validate_all_from(files: &RepositoryFiles) -> Result<Vec<Diagnostic>> {
    validate_all_with_fix(files, false)
}

fn validate_all_with_fix(files: &RepositoryFiles, fix: bool) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = validate_markdown_format(files, fix, None)?;
    let vault = Vault::load_from(files)?;
    let policy_plan = PolicyScanPlan::new(&vault);
    let previous_interface_hashes = previous_architecture_interface_hashes(files)?;
    diagnostics.extend(validate_with_previous_architecture_interfaces(
        &vault,
        previous_interface_hashes.as_ref(),
        &policy_plan,
    ));
    diagnostics.extend(
        policy_plan
            .scan(&vault, None)?
            .into_iter()
            .map(|violation| {
                error_with_location(
                    "policy-violation",
                    &violation.path,
                    Some(violation.line),
                    violation.location,
                    format!(
                        "{} policy `{}` matched `{}`",
                        violation.adr_id, violation.pattern_id, violation.text
                    ),
                )
            }),
    );

    Ok(diagnostics)
}

fn validate_changed(files: &RepositoryFiles) -> Result<Vec<Diagnostic>> {
    let root = files.root();
    let changes = crate::git::staged_changes(root)?;
    if changes.entries.is_empty() {
        return Ok(Vec::new());
    }

    let config = crate::config::Config::load_from(files)?;
    if changed_scope_requires_full_check(&changes, &config.docs_dir, &config.adr_dir) {
        return validate_all_with_fix(files, false);
    }

    let changed_paths = changes
        .affected_paths()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut diagnostics = validate_markdown_format(files, false, Some(&changed_paths))?;
    let vault = Vault::load_from(files)?;
    let policy_plan = PolicyScanPlan::new(&vault);
    let previous_interface_hashes = previous_architecture_interface_hashes(files)?;
    diagnostics.extend(validate_changed_vault(
        &vault,
        previous_interface_hashes.as_ref(),
        &policy_plan,
        &changed_paths,
    ));
    diagnostics.extend(
        policy_plan
            .scan(&vault, Some(&changed_paths))?
            .into_iter()
            .map(|violation| {
                error_with_location(
                    "policy-violation",
                    &violation.path,
                    Some(violation.line),
                    violation.location,
                    format!(
                        "{} policy `{}` matched `{}`",
                        violation.adr_id, violation.pattern_id, violation.text
                    ),
                )
            }),
    );
    diagnostics.sort_by(|a, b| {
        (&a.path, a.line.unwrap_or(0), a.code).cmp(&(&b.path, b.line.unwrap_or(0), b.code))
    });
    Ok(diagnostics)
}

fn changed_scope_requires_full_check(changes: &ChangedSet, docs_dir: &str, adr_dir: &str) -> bool {
    let adr_prefix = format!(
        "{}/{}/",
        docs_dir.trim_end_matches('/'),
        adr_dir.trim_matches('/')
    );
    changes.entries.iter().any(|entry| {
        matches!(
            entry.status,
            ChangeStatus::Deleted | ChangeStatus::Renamed | ChangeStatus::Other
        ) || [
            &entry.path,
            entry.previous_path.as_ref().unwrap_or(&entry.path),
        ]
        .into_iter()
        .any(|path| path == "criv.toml" || path == ".rumdl.toml" || path.starts_with(&adr_prefix))
    })
}

fn validate_markdown_format(
    repository_files: &RepositoryFiles,
    fix: bool,
    changed_paths: Option<&BTreeSet<String>>,
) -> Result<Vec<Diagnostic>> {
    let root = repository_files.root();
    let config = load_rumdl_config(root)?;
    let vault_config = crate::config::Config::load_from(repository_files)?;
    let policy = MarkdownPolicy {
        include: &config.global.include,
        exclude: &config.global.exclude,
        respect_gitignore: config.global.respect_gitignore,
    };
    let files = match changed_paths {
        Some(paths) => select_markdown(root, policy, paths)?,
        None => discover_markdown(root, policy)?,
    };
    let base_rules = base_rules(&config);
    let mut diagnostics = Vec::new();

    for rel_path in files {
        let path = root.join(&rel_path);
        let mut contents = read_selected_text_from(repository_files, &path)?;
        if fix {
            apply_markdown_fixes(
                MarkdownFixScope {
                    files: repository_files,
                    docs_dir: Path::new(&vault_config.docs_dir),
                },
                &path,
                &rel_path,
                &mut contents,
                &config,
                &base_rules,
                &mut diagnostics,
            )?;
        }

        let ignored_rules = config.get_ignored_rules_for_file(&path);
        let result = if ignored_rules.is_empty() {
            rumdl_lib::lint(
                &contents,
                &base_rules,
                false,
                config.get_flavor_for_file(&path),
                Some(path.clone()),
                Some(&config),
            )
        } else {
            let rules = rules_for_ignored_rules(&config, &ignored_rules);
            rumdl_lib::lint(
                &contents,
                &rules,
                false,
                config.get_flavor_for_file(&path),
                Some(path.clone()),
                Some(&config),
            )
        };
        match result {
            Ok(warnings) => {
                let source: Arc<str> = Arc::from(contents.as_str());
                diagnostics.extend(
                    warnings
                        .into_iter()
                        .map(|warning| markdown_diagnostic(&rel_path, source.clone(), warning)),
                );
            }
            Err(err) => diagnostics.push(error(
                "markdown-format",
                &rel_path,
                None,
                format!("rumdl failed: {err}"),
            )),
        }
    }

    Ok(diagnostics)
}

fn load_rumdl_config(root: &Path) -> Result<RumdlConfig> {
    let mut config = match rumdl_lib::config::SourcedConfig::discover_config_for_dir(root, root) {
        Some(path) => rumdl_lib::config::SourcedConfig::load_config_for_path(&path, root)
            .map_err(|err| CrivError::new(format!("failed to load rumdl config: {err}")))?,
        None => RumdlConfig::default(),
    };
    config.project_root = Some(root.to_path_buf());
    Ok(config)
}

#[cfg(test)]
pub(crate) fn discovery_probe_markdown_files(root: &Path) -> Result<Vec<String>> {
    let config = load_rumdl_config(root)?;
    discover_markdown(
        root,
        MarkdownPolicy {
            include: &config.global.include,
            exclude: &config.global.exclude,
            respect_gitignore: config.global.respect_gitignore,
        },
    )
}

fn apply_markdown_fixes(
    write_scope: MarkdownFixScope<'_>,
    path: &Path,
    rel_path: &str,
    contents: &mut String,
    config: &RumdlConfig,
    base_rules: &[Box<dyn Rule>],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    let original = contents.clone();
    let ignored_rules = config.get_ignored_rules_for_file(path);
    let result = if ignored_rules.is_empty() {
        FixCoordinator::new()
            .apply_fixes_iterative(base_rules, &[], contents, config, 10, Some(path))
            .map_err(|err| CrivError::new(format!("rumdl failed to fix {rel_path}: {err}")))?
    } else {
        let rules = rules_for_ignored_rules(config, &ignored_rules);
        FixCoordinator::new()
            .apply_fixes_iterative(&rules, &[], contents, config, 10, Some(path))
            .map_err(|err| CrivError::new(format!("rumdl failed to fix {rel_path}: {err}")))?
    };
    if *contents != original {
        let destination = path.strip_prefix(write_scope.files.root()).map_err(|_| {
            CrivError::new(format!(
                "refusing to fix Markdown outside vault root: {}",
                path.display()
            ))
        })?;
        // Per ADR-0044, `check --fix` rewrites every Markdown file it lints
        // inside the repository root, so the allowed directory tracks the file
        // rather than always being the vault docs directory. Root confinement,
        // symlink rejection, and relative-path validation still apply at `.`;
        // only the docs-subdirectory narrowing is dropped.
        let allowed_dir = if destination.starts_with(write_scope.docs_dir) {
            write_scope.docs_dir
        } else {
            Path::new(".")
        };
        write_scope
            .files
            .write_scope(allowed_dir)?
            .write_atomic(destination, contents)?;
    }

    if !result.converged {
        let detail = if result.conflicting_rules.is_empty() {
            "fix loop did not converge".into()
        } else {
            format!(
                "fix loop did not converge; conflicting rules: {}",
                result.conflicting_rules.join(", ")
            )
        };
        diagnostics.push(error("markdown-format", rel_path, None, detail));
    }

    Ok(())
}

fn base_rules(config: &RumdlConfig) -> Vec<Box<dyn Rule>> {
    filter_rules(&all_rules(config), &config.global)
}

fn rules_for_ignored_rules(
    config: &RumdlConfig,
    ignored_rules: &HashSet<String>,
) -> Vec<Box<dyn Rule>> {
    base_rules(config)
        .into_iter()
        .filter(|rule| !ignored_rules.contains(rule.name()))
        .collect()
}

fn markdown_diagnostic(path: &str, source: Arc<str>, warning: LintWarning) -> Diagnostic {
    let rule = warning.rule_name.as_deref().unwrap_or("rumdl");
    let location = SourceLocation::from_one_based_character_range(
        source,
        warning.line,
        warning.column,
        warning.end_line,
        warning.end_column,
    );
    error_with_location(
        "markdown-format",
        path,
        Some(warning.line),
        location,
        format!("{rule}: {}", warning.message),
    )
}

#[cfg(test)]
fn validate(vault: &Vault) -> Vec<Diagnostic> {
    let policy_plan = PolicyScanPlan::new(vault);
    validate_with_previous_architecture_interfaces(vault, None, &policy_plan)
}

pub(crate) fn validate_with_policy_plan(
    vault: &Vault,
    policy_plan: &PolicyScanPlan,
) -> Vec<Diagnostic> {
    validate_with_previous_architecture_interfaces(vault, None, policy_plan)
}

#[cfg(test)]
pub(crate) fn validate_with_previous_state(
    vault: &Vault,
    previous: Option<&State>,
) -> Vec<Diagnostic> {
    let policy_plan = PolicyScanPlan::new(vault);
    validate_with_previous_state_and_policy_plan(vault, previous, &policy_plan)
}

pub(crate) fn validate_with_previous_state_and_policy_plan(
    vault: &Vault,
    previous: Option<&State>,
    policy_plan: &PolicyScanPlan,
) -> Vec<Diagnostic> {
    let previous_hashes = previous.map(State::architecture_interface_hashes);
    validate_with_previous_architecture_interfaces(vault, previous_hashes.as_ref(), policy_plan)
}

fn validate_with_previous_architecture_interfaces(
    vault: &Vault,
    previous_interface_hashes: Option<&BTreeMap<String, String>>,
    policy_plan: &PolicyScanPlan,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_notes(vault, &mut diagnostics);
    diagnostics.extend(
        policy_plan
            .definition_diagnostics()
            .iter()
            .map(policy_diagnostic),
    );
    validate_pattern_collisions(vault, &mut diagnostics);
    validate_links(vault, &mut diagnostics);
    validate_supersession(vault, &mut diagnostics);
    validate_c4_artifacts(vault, &mut diagnostics);
    validate_likec4_workspace(vault, &mut diagnostics);
    validate_architecture_interface_drift(vault, previous_interface_hashes, &mut diagnostics);
    diagnostics.sort_by(|a, b| {
        (&a.path, a.line.unwrap_or(0), a.code).cmp(&(&b.path, b.line.unwrap_or(0), b.code))
    });
    diagnostics
}

fn validate_likec4_workspace(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.extend(vault.likec4_workspace.diagnostics.iter().map(|diagnostic| {
        error_with_location(
            diagnostic.kind.code(),
            &diagnostic.path,
            diagnostic.line,
            diagnostic.location.clone(),
            diagnostic.message.clone(),
        )
    }));
    let Some(model) = &vault.likec4_workspace.model else {
        return;
    };
    for link in model
        .get("sourceLinks")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(target) = link.get("target").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !matches!(
            vault.resolve_source_target(target),
            SourceTargetResolution::Resolved { .. }
        ) {
            diagnostics.push(error(
                "invalid-likec4-source",
                &vault.likec4_workspace.path,
                None,
                format!("LikeC4 source link does not resolve: `{target}`"),
            ));
        }
    }
}

fn validate_changed_vault(
    vault: &Vault,
    previous_interface_hashes: Option<&BTreeMap<String, String>>,
    policy_plan: &PolicyScanPlan,
    changed_paths: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let adr_prefix = format!("{}/{}/", vault.config.docs_dir, vault.config.adr_dir);
    let adr_readme = format!("{adr_prefix}README.md");

    for note in vault
        .notes
        .iter()
        .filter(|note| changed_paths.contains(&note.rel_path))
    {
        validate_note_local(vault, note, &adr_prefix, &adr_readme, &mut diagnostics);
        validate_note_links(vault, note, &mut diagnostics);
    }
    diagnostics.extend(
        policy_plan
            .definition_diagnostics()
            .iter()
            .filter(|diagnostic| changed_paths.contains(&diagnostic.path))
            .map(policy_diagnostic),
    );
    for artifact in vault
        .c4_artifacts
        .iter()
        .filter(|artifact| changed_paths.contains(&artifact.rel_path))
    {
        validate_c4_artifact(artifact, &mut diagnostics);
    }
    if changed_paths.iter().any(|path| path.ends_with(".c4")) {
        validate_likec4_workspace(vault, &mut diagnostics);
    }
    validate_architecture_interface_drift_for_paths(
        vault,
        previous_interface_hashes,
        Some(changed_paths),
        &mut diagnostics,
    );
    diagnostics.sort_by(|a, b| {
        (&a.path, a.line.unwrap_or(0), a.code).cmp(&(&b.path, b.line.unwrap_or(0), b.code))
    });
    diagnostics
}

fn policy_diagnostic(diagnostic: &PolicyDiagnostic) -> Diagnostic {
    let (code, message) = match &diagnostic.kind {
        PolicyDiagnosticKind::MissingId => (
            "missing-policy-pattern-id",
            "policy pattern must declare an id".to_string(),
        ),
        PolicyDiagnosticKind::EmptyId => (
            "empty-policy-pattern",
            "policy pattern id may not be empty".to_string(),
        ),
        PolicyDiagnosticKind::DuplicateId { id } => (
            "duplicate-policy-pattern",
            format!("policy pattern id `{id}` is declared more than once"),
        ),
        PolicyDiagnosticKind::MissingDefinition { id } => (
            "missing-policy-pattern-definition",
            format!("policy pattern `{id}` must declare language and pattern or rule"),
        ),
        PolicyDiagnosticKind::MissingLanguage { id } => (
            "missing-policy-pattern-language",
            format!("inline policy pattern `{id}` must declare a language"),
        ),
        PolicyDiagnosticKind::AmbiguousBody { id } => (
            "ambiguous-policy-pattern-body",
            format!("inline policy pattern `{id}` must declare either pattern or rule, not both"),
        ),
        PolicyDiagnosticKind::MissingBody { id } => (
            "missing-policy-pattern-body",
            format!("inline policy pattern `{id}` must declare pattern or rule"),
        ),
        PolicyDiagnosticKind::InvalidPattern { id, error } => (
            "invalid-policy-pattern",
            format!("inline policy pattern `{id}` does not compile: {error}"),
        ),
        PolicyDiagnosticKind::InvalidRule { id, error } => (
            "invalid-policy-pattern",
            format!("inline policy rule `{id}` does not compile: {error}"),
        ),
    };
    error(code, &diagnostic.path, Some(diagnostic.line), message)
}

fn previous_architecture_interface_hashes(
    files: &RepositoryFiles,
) -> Result<Option<BTreeMap<String, String>>> {
    let Some(contents) = files.read_optional_string(Path::new(".criv/state.json"))? else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|err| CrivError::new(format!("failed to parse .criv/state.json: {err}")))?;
    let hashes = value
        .pointer("/graph/nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|node| {
            matches!(
                node.get("kind").and_then(serde_json::Value::as_str),
                Some("architecture-interface" | "c4-interface")
            )
        })
        .filter_map(|node| {
            Some((
                node.get("id")?.as_str()?.to_string(),
                node.get("label")?.as_str()?.to_string(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    Ok(Some(hashes))
}

fn validate_notes(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    let mut ids: BTreeMap<&str, Vec<&Note>> = BTreeMap::new();
    let adr_prefix = format!("{}/{}/", vault.config.docs_dir, vault.config.adr_dir);
    let adr_readme = format!("{adr_prefix}README.md");

    for note in &vault.notes {
        validate_note_local(vault, note, &adr_prefix, &adr_readme, diagnostics);
        if let Some(id) = &note.id {
            ids.entry(id).or_default().push(note);
        }
    }

    for (id, notes) in ids {
        if notes.len() > 1 {
            for note in notes {
                diagnostics.push(error(
                    "duplicate-id",
                    &note.rel_path,
                    None,
                    format!("duplicate note id `{id}`"),
                ));
            }
        }
    }
}

fn validate_note_local(
    vault: &Vault,
    note: &Note,
    adr_prefix: &str,
    adr_readme: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(err) = &note.frontmatter_error {
        diagnostics.push(error_with_location(
            "invalid-frontmatter",
            &note.rel_path,
            None,
            note.frontmatter_error_location.clone(),
            err.to_string(),
        ));
    }

    if note.id.is_none() {
        diagnostics.push(error(
            "missing-id",
            &note.rel_path,
            None,
            "note is missing required frontmatter `id`",
        ));
    }

    match note.kind {
        NoteKind::Doc | NoteKind::Decision => {}
        NoteKind::Unknown => diagnostics.push(error(
            "invalid-kind",
            &note.rel_path,
            None,
            "note frontmatter `kind` must be `doc` or `decision`",
        )),
    }

    if note.kind == NoteKind::Decision {
        validate_decision_note(vault, note, adr_prefix, diagnostics);
    } else if note.rel_path.starts_with(adr_prefix) && note.rel_path != adr_readme {
        diagnostics.push(error(
            "adr-dir-non-decision",
            &note.rel_path,
            None,
            "ADR directory may contain only `kind: decision` notes plus README.md",
        ));
    }

    validate_targets(vault, note, diagnostics);
}

fn validate_decision_note(
    vault: &Vault,
    note: &Note,
    adr_prefix: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let id = note.id.as_deref().unwrap_or("");
    if !is_adr_id(id) {
        diagnostics.push(error(
            "invalid-adr-id",
            &note.rel_path,
            None,
            "decision id must match ADR-NNNN",
        ));
    }

    if !note.rel_path.starts_with(adr_prefix) {
        diagnostics.push(error(
            "decision-location",
            &note.rel_path,
            None,
            format!(
                "decision notes must live under {}/{}",
                vault.config.docs_dir, vault.config.adr_dir
            ),
        ));
    }

    if let Some(filename) = note.path.file_name().map(|value| value.to_string_lossy())
        && is_adr_id(id)
    {
        let suffix = &id[4..];
        let expected_prefix = format!("{suffix}-");
        let title_kebab = note.title.as_deref().map(kebab).unwrap_or_default();
        let title_matches = title_kebab.is_empty()
            || filename == format!("{suffix}-{title_kebab}.md")
            || filename.starts_with(&expected_prefix);

        if !title_matches {
            diagnostics.push(error(
                "adr-filename",
                &note.rel_path,
                None,
                format!(
                    "ADR filename should start with `{expected_prefix}` and follow NNNN-kebab-case-title.md"
                ),
            ));
        }
    }
}

fn validate_targets(vault: &Vault, note: &Note, diagnostics: &mut Vec<Diagnostic>) {
    if !vault.is_historical_decision(note) {
        for target in &note.targets_symbols {
            match vault.resolve_source_target(target) {
                SourceTargetResolution::Resolved { .. } => {
                    warn_legacy_source_target(
                        vault,
                        diagnostics,
                        &note.rel_path,
                        None,
                        target,
                        "target symbol",
                    );
                }
                SourceTargetResolution::MissingFile => {
                    diagnostics.push(error(
                        "unresolved-target",
                        &note.rel_path,
                        None,
                        format!("target symbol `{target}` does not resolve to a source file"),
                    ));
                }
                SourceTargetResolution::MissingFragment { path } => {
                    diagnostics.push(error(
                        "unresolved-target",
                        &note.rel_path,
                        None,
                        format!(
                            "target symbol `{target}` resolves to `{path}` but does not resolve to a source symbol"
                        ),
                    ));
                }
            }
        }
    }

    for pattern in &note.target_pattern_refs {
        if vault.resolve_policy_pattern(&pattern.id).is_none() {
            diagnostics.push(error(
                "unresolved-pattern",
                &note.rel_path,
                Some(pattern.line),
                format!("pattern reference `{}` does not resolve", pattern.id),
            ));
        }
    }

    let mut doc_local_patterns = BTreeSet::new();
    for pattern in &note.target_pattern_ids {
        if !doc_local_patterns.insert(pattern) {
            diagnostics.push(error(
                "duplicate-doc-pattern",
                &note.rel_path,
                None,
                format!("doc-local pattern id `{pattern}` is declared more than once"),
            ));
        }
    }

    if !vault.is_historical_decision(note) {
        for (scope, has_match) in note
            .targets_scope
            .iter()
            .zip(vault.source_globs_have_matches(&note.targets_scope))
        {
            if !has_match {
                diagnostics.push(warning(
                    "empty-target-scope",
                    &note.rel_path,
                    None,
                    format!("target scope `{scope}` matches no source files"),
                ));
            }
        }

        for governs in unresolved_governs(vault, note) {
            diagnostics.push(error(
                "unresolved-governs",
                &note.rel_path,
                None,
                format!("governs glob `{governs}` matches no source files"),
            ));
        }
    }
}

fn unresolved_governs(vault: &Vault, note: &Note) -> Vec<String> {
    let governs = vault.effective_governs(note);
    governs
        .iter()
        .zip(vault.source_globs_have_matches(&governs))
        .filter(|(_, has_match)| !has_match)
        .map(|(governs, _)| governs.clone())
        .collect()
}

pub(crate) fn publication_blocking_diagnostics(vault: &Vault) -> Vec<Diagnostic> {
    let mut diagnostics = vault
        .notes
        .iter()
        .filter(|note| vault.is_effective_decision(note))
        .flat_map(|note| {
            unresolved_governs(vault, note).into_iter().map(|governs| {
                error(
                    "unresolved-governs",
                    &note.rel_path,
                    None,
                    format!("governs glob `{governs}` matches no source files"),
                )
            })
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by(|a, b| (&a.path, a.code).cmp(&(&b.path, b.code)));
    diagnostics
}

fn validate_c4_artifacts(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    for artifact in &vault.c4_artifacts {
        validate_c4_artifact(artifact, diagnostics);
    }
}

fn validate_c4_artifact(artifact: &crate::c4::C4Artifact, diagnostics: &mut Vec<Diagnostic>) {
    for artifact_diagnostic in &artifact.diagnostics {
        diagnostics.push(error_with_location(
            artifact_diagnostic.code,
            &artifact.rel_path,
            artifact_diagnostic.line,
            artifact_diagnostic.location.clone(),
            artifact_diagnostic.message.clone(),
        ));
    }
}

fn validate_architecture_interface_drift(
    vault: &Vault,
    previous_interface_hashes: Option<&BTreeMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    validate_architecture_interface_drift_for_paths(
        vault,
        previous_interface_hashes,
        None,
        diagnostics,
    );
}

fn validate_architecture_interface_drift_for_paths(
    vault: &Vault,
    previous_interface_hashes: Option<&BTreeMap<String, String>>,
    changed_paths: Option<&BTreeSet<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(previous_interface_hashes) = previous_interface_hashes else {
        return;
    };
    for record in state::architecture_interface_hash_records(vault) {
        if changed_paths.is_some_and(|paths| {
            !paths.contains(&record.source_path)
                && !paths.iter().any(|path| {
                    path == &record.path || path.starts_with(&format!("{}/", record.path))
                })
        }) {
            continue;
        }
        let Some(previous_hash) = previous_interface_hashes.get(&record.id) else {
            continue;
        };
        if previous_hash != &record.hash {
            diagnostics.push(warning(
                "architecture-interface-drift",
                &record.path,
                Some(record.line),
                format!(
                    "LikeC4 source `{}` interface changed since the previous state; review the architecture model",
                    record.target
                ),
            ));
        }
    }
}

fn validate_pattern_collisions(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    let mut declarations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for note in &vault.notes {
        for pattern in &note.target_pattern_ids {
            declarations
                .entry(pattern.clone())
                .or_default()
                .push(note.rel_path.clone());
        }
        if let Some(id) = &note.id {
            for pattern in &note.policy_patterns {
                if let Some(pattern_id) = pattern.id.as_deref() {
                    declarations
                        .entry(format!("{id}/{pattern_id}"))
                        .or_default()
                        .push(note.rel_path.clone());
                }
            }
        }
    }

    for (pattern, locations) in declarations {
        if locations.len() <= 1 {
            continue;
        }
        for location in &locations {
            diagnostics.push(error(
                "duplicate-pattern-id",
                location,
                None,
                format!(
                    "pattern id `{pattern}` is declared in multiple places: {}",
                    locations.join(", ")
                ),
            ));
        }
    }
}

fn validate_links(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    for note in &vault.notes {
        validate_note_links(vault, note, diagnostics);
    }
}

fn validate_note_links(vault: &Vault, note: &Note, diagnostics: &mut Vec<Diagnostic>) {
    for link in &note.wiki_links {
        match vault.resolve_link(&link.target) {
            ResolvedLink::Broken => diagnostics.push(error_with_location(
                "broken-link",
                &note.rel_path,
                Some(link.line),
                link.location.clone(),
                format!("wiki-link `[[{}]]` does not resolve", link.raw),
            )),
            ResolvedLink::Source { path, ambiguous } => {
                let suggestion = vault
                    .canonical_source_target_for_path(&link.target, &path)
                    .map(|target| {
                        format!("; use AST-aware source selector `{target}` for code references")
                    })
                    .unwrap_or_default();
                diagnostics.push(warning_with_location(
                        "source-wikilink",
                        &note.rel_path,
                        Some(link.line),
                        link.location.clone(),
                        format!(
                            "wiki-link `[[{}]]` targets source `{path}`; Wikilinks are reserved for document references{suggestion}",
                            link.raw
                        ),
                    ));
                if ambiguous {
                    diagnostics.push(warning_with_location(
                        "ambiguous-source-link",
                        &note.rel_path,
                        Some(link.line),
                        link.location.clone(),
                        format!(
                            "wiki-link `[[{}]]` resolves ambiguously; first match is `{path}`",
                            link.raw
                        ),
                    ));
                }
            }
            ResolvedLink::Pattern { .. } => {}
            ResolvedLink::Note { .. } => {
                if !vault.is_file_backed_note_target(&link.target) {
                    let suggestion = vault
                        .portable_note_target(&link.target)
                        .map(|target| format!("; use `[[{target}]]` instead"))
                        .unwrap_or_default();
                    diagnostics.push(error_with_location(
                            "non-portable-note-link",
                            &note.rel_path,
                            Some(link.line),
                            link.location.clone(),
                            format!(
                                "wiki-link `[[{}]]` resolves through note metadata but does not target an existing markdown file{suggestion}",
                                link.raw
                            ),
                        ));
                }
            }
        }
    }
}

fn warn_legacy_source_target(
    vault: &Vault,
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    line: Option<usize>,
    target: &str,
    label: &str,
) {
    let Some(canonical) = vault.canonical_source_target(target) else {
        return;
    };
    let normalized = source_target_body(target);
    if is_typed_source_target(target) || normalized != canonical {
        diagnostics.push(warning(
            "legacy-source-target",
            path,
            line,
            format!(
                "{label} `{target}` is a legacy source target; use AST-aware source selector `{canonical}`"
            ),
        ));
    }
}

fn validate_supersession(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    let decisions = vault
        .notes
        .iter()
        .filter(|note| note.kind == NoteKind::Decision)
        .filter_map(|note| note.id.as_deref().map(|id| (id, note)))
        .collect::<BTreeMap<_, _>>();

    for (id, note) in &decisions {
        for old_id in &note.supersedes {
            if !decisions.contains_key(old_id.as_str()) {
                diagnostics.push(error(
                    "unknown-supersedes",
                    &note.rel_path,
                    None,
                    format!("supersedes references unknown decision `{old_id}`"),
                ));
            }
        }

        for new_id in &note.superseded_by {
            match decisions.get(new_id.as_str()) {
                None => diagnostics.push(error(
                    "unknown-superseded-by",
                    &note.rel_path,
                    None,
                    format!("superseded_by references unknown decision `{new_id}`"),
                )),
                Some(new_note) => {
                    if !new_note.supersedes.iter().any(|value| value == id) {
                        diagnostics.push(error(
                            "inconsistent-supersession",
                            &note.rel_path,
                            None,
                            format!("`{new_id}` must list `{id}` in supersedes"),
                        ));
                    }
                }
            }
        }
    }

    for cycle in supersession_cycles(&decisions) {
        diagnostics.push(error(
            "supersession-cycle",
            decisions
                .get(cycle.first().map(String::as_str).unwrap_or(""))
                .map(|note| note.rel_path.as_str())
                .unwrap_or("docs"),
            None,
            format!("supersession chain is cyclic: {}", cycle.join(" -> ")),
        ));
    }
}

fn supersession_cycles(decisions: &BTreeMap<&str, &Note>) -> Vec<Vec<String>> {
    let mut cycles = Vec::new();
    for id in decisions.keys() {
        let mut seen = BTreeSet::new();
        let mut stack = Vec::new();
        visit_supersession(id, decisions, &mut seen, &mut stack, &mut cycles);
    }
    cycles.sort();
    cycles.dedup();
    cycles
}

fn visit_supersession(
    id: &str,
    decisions: &BTreeMap<&str, &Note>,
    seen: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    if let Some(position) = stack.iter().position(|value| value == id) {
        let mut cycle = stack[position..].to_vec();
        cycle.push(id.to_string());
        cycles.push(cycle);
        return;
    }
    if !seen.insert(id.to_string()) {
        return;
    }

    stack.push(id.to_string());
    if let Some(note) = decisions.get(id) {
        for next in &note.supersedes {
            visit_supersession(next, decisions, seen, stack, cycles);
        }
    }
    stack.pop();
}

fn print_text(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        println!("criv check: ok");
        return;
    }

    for diag in diagnostics {
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if let Some(line) = diag.line {
            println!(
                "{severity}[{}] {}:{line}: {}",
                diag.code, diag.path, diag.message
            );
        } else {
            println!("{severity}[{}] {}: {}", diag.code, diag.path, diag.message);
        }
    }
}

fn print_json(diagnostics: &[Diagnostic]) -> Result<()> {
    let diagnostics = diagnostics.iter().map(Diagnostic::json).collect::<Vec<_>>();
    let json = serde_json::to_string_pretty(&diagnostics)
        .map_err(|err| CrivError::new(format!("failed to serialize check diagnostics: {err}")))?;
    println!("{json}");
    Ok(())
}

fn print_github(diagnostics: &[Diagnostic]) {
    for diag in diagnostics {
        println!("{}", github_annotation(diag));
    }
}

fn github_annotation(diag: &Diagnostic) -> String {
    let command = match diag.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let location = if let Some(location) = &diag.location {
        let location = location.github_location();
        match (location.column, location.end_column) {
            (Some(column), Some(end_column)) => format!(
                ",line={},col={column},endLine={},endColumn={end_column}",
                location.line, location.end_line
            ),
            _ => format!(",line={},endLine={}", location.line, location.end_line),
        }
    } else {
        diag.line
            .map(|line| format!(",line={line}"))
            .unwrap_or_default()
    };
    format!(
        "::{command} file={}{},title={}::{}",
        escape_github_property(&diag.path),
        location,
        escape_github_property(&format!("criv {}", diag.code)),
        escape_github_message(&diag.message)
    )
}

fn escape_github_message(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_github_property(value: &str) -> String {
    escape_github_message(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn error(
    code: &'static str,
    path: &str,
    line: Option<usize>,
    message: impl Into<String>,
) -> Diagnostic {
    error_with_location(code, path, line, None, message)
}

fn error_with_location(
    code: &'static str,
    path: &str,
    line: Option<usize>,
    location: Option<SourceLocation>,
    message: impl Into<String>,
) -> Diagnostic {
    let line = location.as_ref().map(SourceLocation::line).or(line);
    Diagnostic {
        severity: Severity::Error,
        code,
        path: path.into(),
        line,
        message: message.into(),
        location,
    }
}

fn warning(
    code: &'static str,
    path: &str,
    line: Option<usize>,
    message: impl Into<String>,
) -> Diagnostic {
    warning_with_location(code, path, line, None, message)
}

fn warning_with_location(
    code: &'static str,
    path: &str,
    line: Option<usize>,
    location: Option<SourceLocation>,
    message: impl Into<String>,
) -> Diagnostic {
    let line = location.as_ref().map(SourceLocation::line).or(line);
    Diagnostic {
        severity: Severity::Warning,
        code,
        path: path.into(),
        line,
        message: message.into(),
        location,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::WikiLink;

    fn changed_set_fixture(entries: Vec<ChangedEntry>) -> ChangedSet {
        ChangedSet {
            entries,
            old_ref: Some("HEAD".into()),
            new_ref: Some(":".into()),
            basis: "test".into(),
        }
    }

    fn changed_entry(
        status: ChangeStatus,
        path: &str,
        previous_path: Option<&str>,
    ) -> ChangedEntry {
        ChangedEntry {
            status,
            path: path.into(),
            previous_path: previous_path.map(str::to_string),
            old_ref: Some("HEAD".into()),
            new_ref: Some(":".into()),
        }
    }

    #[test]
    fn changed_scope_keeps_safe_additions_and_modifications_partial() {
        let changes = changed_set_fixture(vec![
            changed_entry(ChangeStatus::Added, "docs/guide.md", None),
            changed_entry(ChangeStatus::Modified, "src/lib.rs", None),
        ]);

        assert!(!changed_scope_requires_full_check(&changes, "docs", "adr"));
    }

    #[test]
    fn changed_scope_promotes_global_transactions_to_full_check() {
        for entry in [
            changed_entry(ChangeStatus::Deleted, "docs/guide.md", None),
            changed_entry(ChangeStatus::Renamed, "docs/new.md", Some("docs/old.md")),
            changed_entry(ChangeStatus::Modified, "docs/adr/0067-decision.md", None),
            changed_entry(ChangeStatus::Modified, "criv.toml", None),
            changed_entry(ChangeStatus::Modified, ".rumdl.toml", None),
        ] {
            assert!(changed_scope_requires_full_check(
                &changed_set_fixture(vec![entry]),
                "docs",
                "adr"
            ));
        }
    }

    #[test]
    fn affected_paths_include_both_sides_of_a_rename() {
        let changes = changed_set_fixture(vec![changed_entry(
            ChangeStatus::Renamed,
            "docs/new.md",
            Some("docs/old.md"),
        )]);

        assert_eq!(
            changes.affected_paths(),
            vec!["docs/new.md".to_string(), "docs/old.md".to_string()]
        );
    }

    #[test]
    fn github_annotation_escapes_workflow_command_data() {
        let diag = Diagnostic {
            severity: Severity::Error,
            code: "broken-link",
            path: "docs/a,b:guide.md".into(),
            line: Some(7),
            message: "bad % link\r\ntry again".into(),
            location: None,
        };

        assert_eq!(
            github_annotation(&diag),
            "::error file=docs/a%2Cb%3Aguide.md,line=7,title=criv broken-link::bad %25 link%0D%0Atry again"
        );
    }

    #[test]
    fn github_annotation_omits_missing_line() {
        let diag = Diagnostic {
            severity: Severity::Warning,
            code: "missing-id",
            path: "docs/note.md".into(),
            line: None,
            message: "missing id".into(),
            location: None,
        };

        assert_eq!(
            github_annotation(&diag),
            "::warning file=docs/note.md,title=criv missing-id::missing id"
        );
        let json = serde_json::to_value(diag.json()).unwrap();
        assert!(json["line"].is_null());
        assert!(json.get("range").is_none());
    }

    #[test]
    fn exact_diagnostics_serialize_lsp_ranges_and_github_columns() {
        let diagnostic = markdown_diagnostic(
            "docs/unicode.md",
            Arc::from("é😀bad\n"),
            LintWarning {
                message: "bad emoji".into(),
                line: 1,
                column: 2,
                end_line: 1,
                end_column: 3,
                severity: rumdl_lib::rule::Severity::Warning,
                fix: None,
                rule_name: Some("MD999".into()),
            },
        );

        let json = serde_json::to_value(diagnostic.json()).unwrap();
        assert_eq!(
            json["range"],
            serde_json::json!({
                "start": { "line": 0, "character": 1 },
                "end": { "line": 0, "character": 3 }
            })
        );
        assert_eq!(
            github_annotation(&diagnostic),
            "::error file=docs/unicode.md,line=1,col=2,endLine=1,endColumn=2,title=criv markdown-format::MD999: bad emoji"
        );
    }

    #[test]
    fn invalid_exact_locations_keep_the_line_only_shape() {
        let diagnostic = markdown_diagnostic(
            "docs/invalid.md",
            Arc::from("short\n"),
            LintWarning {
                message: "invalid range".into(),
                line: 1,
                column: 20,
                end_line: 1,
                end_column: 21,
                severity: rumdl_lib::rule::Severity::Warning,
                fix: None,
                rule_name: Some("MD999".into()),
            },
        );

        let json = serde_json::to_value(diagnostic.json()).unwrap();
        assert!(json.get("range").is_none());
        assert_eq!(
            github_annotation(&diagnostic),
            "::error file=docs/invalid.md,line=1,title=criv markdown-format::MD999: invalid range"
        );
    }

    #[test]
    fn multiline_github_annotations_use_lines_without_unsupported_columns() {
        let source: Arc<str> = Arc::from("first\nsecond\n");
        let location = SourceLocation::new(source, 2..9).unwrap();
        let diagnostic = error_with_location(
            "multi-line",
            "docs/multi.md",
            None,
            Some(location),
            "crosses lines",
        );

        assert_eq!(
            github_annotation(&diagnostic),
            "::error file=docs/multi.md,line=1,endLine=2,title=criv multi-line::crosses lines"
        );
    }

    #[test]
    fn cycles_are_detected() {
        let mut a = empty_decision("ADR-0001");
        let mut b = empty_decision("ADR-0002");
        a.supersedes.push("ADR-0002".into());
        b.supersedes.push("ADR-0001".into());
        let decisions = BTreeMap::from([("ADR-0001", &a), ("ADR-0002", &b)]);
        assert!(!supersession_cycles(&decisions).is_empty());
    }

    #[test]
    fn supersedes_does_not_require_old_adr_backlink() {
        let mut old = empty_decision("ADR-0001");
        let mut new = empty_decision("ADR-0002");
        new.supersedes.push("ADR-0001".into());
        let vault = test_vault(vec![old.clone(), new]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "inconsistent-supersession")
        );
        old.superseded_by.push("ADR-0003".into());
        let vault = test_vault(vec![old]);
        assert!(
            validate(&vault)
                .iter()
                .any(|diag| diag.code == "unknown-superseded-by")
        );
    }

    #[test]
    fn metadata_only_note_links_are_non_portable() {
        let target = decision_note("ADR-0001", "docs/adr/0001-local-cli-vault-architecture.md");
        let mut source = decision_note("ADR-0002", "docs/adr/0002-context.md");
        source.wiki_links.push(wiki_link("ADR-0001"));
        let vault = test_vault(vec![target, source]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "non-portable-note-link")
        );
    }

    #[test]
    fn file_backed_note_links_are_portable() {
        let target = decision_note("ADR-0001", "docs/adr/0001-local-cli-vault-architecture.md");
        let mut source = decision_note("ADR-0002", "docs/adr/0002-context.md");
        source
            .wiki_links
            .push(wiki_link("0001-local-cli-vault-architecture|ADR-0001"));
        let vault = test_vault(vec![target, source]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "non-portable-note-link")
        );
    }

    #[test]
    fn source_wikilinks_are_reported_as_document_portability_issues() {
        let vault = source_note_vault("[[src/main.rs#fn:run]]", &[]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| {
            diag.code == "source-wikilink"
                && diag
                    .message
                    .contains("AST-aware source selector `src/main.rs#fn:run`")
        }));
        assert!(diagnostics.iter().all(|diag| diag.code != "broken-link"));
    }

    #[test]
    fn adr_0033_source_wikilinks_remain_compatible_but_warn() {
        let vault = source_note_vault("[[source:src/main.rs#run]]", &[]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| {
            diag.code == "source-wikilink"
                && diag
                    .message
                    .contains("AST-aware source selector `src/main.rs#fn:run`")
        }));
        assert!(diagnostics.iter().all(|diag| diag.code != "broken-link"));
    }

    #[test]
    fn typed_source_wikilinks_with_missing_fragments_remain_broken() {
        let vault = source_note_vault("[[source:src/main.rs#missing]]", &[]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| diag.code == "broken-link"));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "source-wikilink")
        );
    }

    #[test]
    fn ambiguous_legacy_source_wikilinks_retain_both_diagnostics() {
        let vault = source_note_vault("[[shared.rs]]", &[]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "source-wikilink"),
            "{diagnostics:#?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "ambiguous-source-link"
                    && diag.message.contains("first match is `")),
            "{diagnostics:#?}"
        );
        assert!(diagnostics.iter().all(|diag| diag.code != "broken-link"));
    }

    #[test]
    fn legacy_target_symbols_suggest_ast_aware_selectors() {
        let vault = source_note_vault("", &["src/main.rs#run"]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| {
            diag.code == "legacy-source-target"
                && diag
                    .message
                    .contains("AST-aware source selector `src/main.rs#fn:run`")
        }));
    }

    #[test]
    fn canonical_ast_aware_target_symbols_do_not_warn() {
        let vault = source_note_vault("", &["src/main.rs#fn:run"]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "legacy-source-target")
        );
    }

    #[test]
    fn ast_aware_governs_selectors_resolve() {
        let root = unique_temp_dir("criv-governs-selector-check");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        fs::write(
            root.join("docs/adr/0999-governs-selector.md"),
            r#"---
id: ADR-0999
kind: decision
title: Governs Selector
status: accepted
governs:
  - src/main.rs#fn:run
---

# Governs Selector
"#,
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "unresolved-governs")
        );
    }

    #[test]
    fn historical_accepted_adrs_do_not_require_current_source_bindings() {
        let root = unique_temp_dir("criv-historical-source-bindings");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/current.rs"), "fn current() {}\n").unwrap();
        fs::write(
            root.join("docs/adr/0001-old.md"),
            r#"---
id: ADR-0001
kind: decision
title: Old
status: accepted
governs:
  - src/removed.rs
targets:
  symbols:
    - src/removed.rs#fn:removed
  scope:
    - src/removed.rs
---

# Old
"#,
        )
        .unwrap();
        fs::write(
            root.join("docs/adr/0002-successor.md"),
            r#"---
id: ADR-0002
kind: decision
title: Successor
status: accepted
supersedes:
  - ADR-0001
governs:
  - src/current.rs
---

# Successor
"#,
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "unresolved-governs"
                && diagnostic.code != "unresolved-target"
                && diagnostic.code != "empty-target-scope"
        }));
        assert!(publication_blocking_diagnostics(&vault).is_empty());
    }

    #[test]
    fn draft_successor_does_not_suppress_active_governance_failure() {
        let root = unique_temp_dir("criv-draft-successor-governance");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/current.rs"), "fn current() {}\n").unwrap();
        fs::write(
            root.join("docs/adr/0001-old.md"),
            r#"---
id: ADR-0001
kind: decision
title: Old
status: accepted
governs:
  - src/removed.rs
---

# Old
"#,
        )
        .unwrap();
        fs::write(
            root.join("docs/adr/0002-successor.md"),
            r#"---
id: ADR-0002
kind: decision
title: Draft successor
status: draft
supersedes:
  - ADR-0001
governs:
  - src/current.rs
---

# Draft successor
"#,
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();

        let blockers = publication_blocking_diagnostics(&vault);

        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].code, "unresolved-governs");
        assert_eq!(blockers[0].path, "docs/adr/0001-old.md");
    }

    #[test]
    fn broken_link_diagnostics_report_the_real_file_line() {
        let root = unique_temp_dir("criv-broken-link-line");
        fs::create_dir_all(root.join("docs")).unwrap();
        let note = r#"---
id: DOC-LINE
kind: doc
title: Line Check
---

# Line Check

See [[does-not-exist]].
"#;
        fs::write(root.join("criv.toml"), "[source]\nindex = false\n").unwrap();
        fs::write(root.join("docs/line-check.md"), note).unwrap();
        let vault = Vault::load(&root).unwrap();

        let diagnostics = validate(&vault);

        let expected = note
            .lines()
            .position(|line| line.contains("[[does-not-exist]]"))
            .unwrap()
            + 1;
        let broken = diagnostics
            .iter()
            .find(|diag| diag.code == "broken-link")
            .expect("the dangling wiki link must be reported");
        assert_eq!(
            broken.line,
            Some(expected),
            "the diagnostic must point at the real file line, not a body-relative one"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn likec4_interface_drift_ignores_body_only_changes() {
        let root = architecture_interface_drift_fixture(
            r#"
pub fn run(input: String) -> usize {
  input.len()
}
"#,
        );
        let previous_vault = likec4_interface_vault(&root);
        let previous_state = State::build(&previous_vault).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            r#"
pub fn run(input: String) -> usize {
  println!("{}", input);
  42
}
"#,
        )
        .unwrap();

        let vault = likec4_interface_vault(&root);
        let diagnostics = validate_with_previous_state(&vault, Some(&previous_state));

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "architecture-interface-drift")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn likec4_interface_drift_warns_on_signature_changes() {
        let root = architecture_interface_drift_fixture(
            r#"
pub fn run(input: String) -> usize {
  input.len()
}
"#,
        );
        let previous_vault = likec4_interface_vault(&root);
        let previous_state = State::build(&previous_vault).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            r#"
pub fn run(input: String, fallback: usize) -> usize {
  fallback
}
"#,
        )
        .unwrap();

        let vault = likec4_interface_vault(&root);
        let diagnostics = validate_with_previous_state(&vault, Some(&previous_state));

        assert!(diagnostics.iter().any(|diag| {
            diag.code == "architecture-interface-drift"
                && diag.path == "docs/architecture"
                && diag.is_warning()
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rumdl_fixes_markdown_content_in_process() {
        let path = unique_temp_file("criv-rumdl-fix", "README.md");
        let root = path.parent().unwrap();
        let mut contents = "# Title\n\n\nBody\n".to_string();
        fs::write(&path, &contents).unwrap();
        let config = RumdlConfig::default();
        let base_rules = base_rules(&config);
        let mut diagnostics = Vec::new();
        let files = RepositoryFiles::open(root).unwrap();
        let path = files.root().join("README.md");

        apply_markdown_fixes(
            MarkdownFixScope {
                files: &files,
                docs_dir: Path::new("."),
            },
            &path,
            "README.md",
            &mut contents,
            &config,
            &base_rules,
            &mut diagnostics,
        )
        .unwrap();

        assert!(diagnostics.is_empty());
        assert_eq!(contents, "# Title\n\nBody\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }

    fn unique_temp_file(prefix: &str, name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{unique}-{counter}",
            std::process::id()
        ))
    }

    fn empty_decision(id: &str) -> Note {
        Note {
            path: id.into(),
            rel_path: format!("adr/{id}.md"),
            id: Some(id.into()),
            kind: NoteKind::Decision,
            title: None,
            status: None,
            body: String::new(),
            headings: Vec::new(),
            targets_symbols: Vec::new(),
            targets_scope: Vec::new(),
            target_pattern_refs: Vec::new(),
            target_pattern_ids: Vec::new(),
            policy_patterns: Vec::new(),
            governs: Vec::new(),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            wiki_links: Vec::new(),
            frontmatter_error: None,
            frontmatter_error_location: None,
        }
    }

    fn decision_note(id: &str, rel_path: &str) -> Note {
        let mut note = empty_decision(id);
        note.path = rel_path.into();
        note.rel_path = rel_path.into();
        note
    }

    fn wiki_link(target: &str) -> WikiLink {
        WikiLink {
            raw: target.into(),
            target: target.into(),
            line: 1,
            location: None,
        }
    }

    fn test_vault(notes: Vec<Note>) -> Vault {
        Vault::from_parts_for_test(notes)
    }

    fn source_note_vault(body: &str, target_symbols: &[&str]) -> Vault {
        let root = unique_temp_dir("criv-source-link-check");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        fs::create_dir_all(root.join("src/one")).unwrap();
        fs::create_dir_all(root.join("src/two")).unwrap();
        fs::write(root.join("src/one/shared.rs"), "fn one() {}\n").unwrap();
        fs::write(root.join("src/two/shared.rs"), "fn two() {}\n").unwrap();
        let targets = if target_symbols.is_empty() {
            String::new()
        } else {
            format!(
                "targets:\n  symbols:\n{}\n",
                target_symbols
                    .iter()
                    .map(|target| format!("    - {target}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        fs::write(
            root.join("docs/note.md"),
            format!(
                r#"---
id: source-note
kind: doc
title: Source Note
{targets}---

# Source Note

{body}
"#
            ),
        )
        .unwrap();
        Vault::load(&root).unwrap()
    }

    fn architecture_interface_drift_fixture(source: &str) -> std::path::PathBuf {
        let root = unique_temp_dir("criv-c4-interface-drift");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/architecture")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), source).unwrap();
        root
    }

    fn likec4_interface_vault(root: &Path) -> Vault {
        let mut vault = Vault::load(root).unwrap();
        vault.likec4_workspace.path = "docs/architecture".into();
        vault.likec4_workspace.model = Some(serde_json::json!({
            "elements": [{ "id": "cli", "title": "criv CLI" }],
            "relationships": [],
            "sourceLinks": [{
                "element": "cli",
                "target": "src/lib.rs#fn:run"
            }]
        }));
        vault
    }
}

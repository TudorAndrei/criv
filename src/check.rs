use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use clap::{Args as ClapArgs, ValueEnum};
use ignore::WalkBuilder;
use rumdl_lib::config::Config as RumdlConfig;
use rumdl_lib::fix_coordinator::FixCoordinator;
use rumdl_lib::rule::{LintWarning, Rule};
use rumdl_lib::rules::{all_rules, filter_rules};
use serde::Serialize;

use crate::c4::{C4ElementCategory, C4Level};
use crate::c4_artifact::C4ArtifactFormat;
use crate::state::{self, State};
use crate::util::{is_adr_id, kebab};
use crate::vault::{
    Note, NoteKind, PolicyPattern, ResolvedLink, SourceTargetResolution, Vault,
    is_typed_source_target, source_target_body,
};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct CheckOptions {
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long)]
    fix: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Diagnostic {
    severity: Severity,
    code: &'static str,
    path: String,
    line: Option<usize>,
    message: String,
}

impl Diagnostic {
    pub(crate) fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }

    pub(crate) fn is_warning(&self) -> bool {
        matches!(self.severity, Severity::Warning)
    }
}

pub(crate) fn run(root: &Path, options: CheckOptions) -> Result<()> {
    let mut diagnostics = validate_markdown_format(root, options.fix)?;
    let vault = Vault::load(root)?;
    let previous_interface_hashes = previous_c4_interface_hashes(root)?;
    diagnostics.extend(validate_with_previous_c4_interfaces(
        &vault,
        previous_interface_hashes.as_ref(),
    ));
    diagnostics.extend(
        policy_violations(root, &vault)?
            .into_iter()
            .map(|violation| {
                error(
                    "policy-violation",
                    &violation.path,
                    violation.line,
                    format!(
                        "{} policy `{}` matched `{}`",
                        violation.adr_id, violation.pattern_id, violation.text
                    ),
                )
            }),
    );

    if let Some(filter) = &options.filter {
        diagnostics.retain(|diag| {
            diag.path.contains(filter)
                || diag.message.contains(filter)
                || diag.code.contains(filter)
        });
    }

    match options.format {
        Format::Text => print_text(&diagnostics),
        Format::Json => print_json(&diagnostics)?,
    }

    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(CrivError::new("check failed"));
    }

    Ok(())
}

fn validate_markdown_format(root: &Path, fix: bool) -> Result<Vec<Diagnostic>> {
    let config = load_rumdl_config(root)?;
    let files = markdown_files(root, &config);
    let mut diagnostics = Vec::new();

    for rel_path in files {
        let path = root.join(&rel_path);
        let mut contents = crate::util::read_to_string(&path)?;
        if fix {
            apply_markdown_fixes(&path, &rel_path, &mut contents, &config, &mut diagnostics)?;
        }

        let rules = rules_for_file(&path, &config);
        match rumdl_lib::lint(
            &contents,
            &rules,
            false,
            config.get_flavor_for_file(&path),
            Some(path.clone()),
            Some(&config),
        ) {
            Ok(warnings) => {
                diagnostics.extend(
                    warnings
                        .into_iter()
                        .map(|warning| markdown_diagnostic(&rel_path, warning)),
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

fn markdown_files(root: &Path, config: &RumdlConfig) -> Vec<String> {
    let mut files = WalkBuilder::new(root)
        .git_ignore(config.global.respect_gitignore)
        .git_global(config.global.respect_gitignore)
        .git_exclude(config.global.respect_gitignore)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .filter_map(|entry| {
            let path = entry.into_path();
            is_markdown_file(&path).then(|| relative_path(root, &path))
        })
        .filter(|path| {
            config.global.include.is_empty()
                || config
                    .global
                    .include
                    .iter()
                    .any(|pattern| crate::util::glob_matches(pattern, path))
        })
        .filter(|path| {
            !config
                .global
                .exclude
                .iter()
                .any(|pattern| crate::util::glob_matches(pattern, path))
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn apply_markdown_fixes(
    path: &Path,
    rel_path: &str,
    contents: &mut String,
    config: &RumdlConfig,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    let rules = rules_for_file(path, config);
    let original = contents.clone();
    let result = FixCoordinator::new()
        .apply_fixes_iterative(&rules, &[], contents, config, 10, Some(path))
        .map_err(|err| CrivError::new(format!("rumdl failed to fix {rel_path}: {err}")))?;
    if *contents != original {
        fs::write(path, contents.as_bytes())?;
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

fn rules_for_file(path: &Path, config: &RumdlConfig) -> Vec<Box<dyn Rule>> {
    let rules = filter_rules(&all_rules(config), &config.global);
    let ignored_rules = config.get_ignored_rules_for_file(path);
    if ignored_rules.is_empty() {
        return rules;
    }

    rules
        .into_iter()
        .filter(|rule| !ignored_rules.contains(rule.name()))
        .collect()
}

fn markdown_diagnostic(path: &str, warning: LintWarning) -> Diagnostic {
    let rule = warning.rule_name.as_deref().unwrap_or("rumdl");
    error(
        "markdown-format",
        path,
        Some(warning.line),
        format!("{rule}: {}", warning.message),
    )
}

fn is_markdown_file(path: &Path) -> bool {
    mime_guess::from_path(path)
        .first()
        .is_some_and(|mime| mime.essence_str() == "text/markdown")
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

struct PolicyViolation {
    path: String,
    line: Option<usize>,
    adr_id: String,
    pattern_id: String,
    text: String,
}

fn policy_violations(root: &Path, vault: &Vault) -> Result<Vec<PolicyViolation>> {
    let mut violations = Vec::new();
    for note in &vault.notes {
        if note.status.as_deref() != Some("accepted") {
            continue;
        }
        let Some(adr_id) = &note.id else {
            continue;
        };
        let scopes = policy_scope_files(vault, &vault.effective_governs(note));
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
            let pattern_id = format!("{adr_id}/{local_id}");
            let rows = crate::structural::find_policy_pattern_entry(root, vault, pattern, &scopes)?;
            violations.extend(rows.into_iter().map(|row| PolicyViolation {
                path: row.path,
                line: Some(row.line),
                adr_id: adr_id.clone(),
                pattern_id: pattern_id.clone(),
                text: row.text,
            }));
        }
    }
    Ok(violations)
}

fn policy_scope_files(vault: &Vault, scopes: &[String]) -> Vec<String> {
    scopes
        .iter()
        .flat_map(|scope| vault.source_files_matching_glob(scope))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn validate(vault: &Vault) -> Vec<Diagnostic> {
    validate_with_previous_c4_interfaces(vault, None)
}

pub(crate) fn validate_with_previous_state(
    vault: &Vault,
    previous: Option<&State>,
) -> Vec<Diagnostic> {
    let previous_hashes = previous.map(State::c4_interface_hashes);
    validate_with_previous_c4_interfaces(vault, previous_hashes.as_ref())
}

fn validate_with_previous_c4_interfaces(
    vault: &Vault,
    previous_interface_hashes: Option<&BTreeMap<String, String>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_notes(vault, &mut diagnostics);
    validate_pattern_collisions(vault, &mut diagnostics);
    validate_links(vault, &mut diagnostics);
    validate_supersession(vault, &mut diagnostics);
    validate_c4_artifacts(vault, &mut diagnostics);
    validate_c4_interface_drift(vault, previous_interface_hashes, &mut diagnostics);
    diagnostics.sort_by(|a, b| {
        (&a.path, a.line.unwrap_or(0), a.code).cmp(&(&b.path, b.line.unwrap_or(0), b.code))
    });
    diagnostics
}

fn previous_c4_interface_hashes(root: &Path) -> Result<Option<BTreeMap<String, String>>> {
    let path = root.join(".criv/state.json");
    if !path.exists() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)
        .map_err(|err| CrivError::new(format!("failed to parse .criv/state.json: {err}")))?;
    let hashes = value
        .pointer("/graph/nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|node| node.get("kind").and_then(serde_json::Value::as_str) == Some("c4-interface"))
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
        if let Some(err) = &note.frontmatter_error {
            diagnostics.push(error(
                "invalid-frontmatter",
                &note.rel_path,
                None,
                err.to_string(),
            ));
        }

        if let Some(id) = &note.id {
            ids.entry(id).or_default().push(note);
        } else {
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
            validate_decision_note(vault, note, &adr_prefix, diagnostics);
        } else if note.rel_path.starts_with(&adr_prefix) && note.rel_path != adr_readme {
            diagnostics.push(error(
                "adr-dir-non-decision",
                &note.rel_path,
                None,
                "ADR directory may contain only `kind: decision` notes plus README.md",
            ));
        }

        validate_targets(vault, note, diagnostics);
        validate_c4_diagrams(vault, &note.rel_path, &note.c4_diagrams, diagnostics);
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

    validate_policy_patterns(note, diagnostics);
}

fn validate_policy_patterns(note: &Note, diagnostics: &mut Vec<Diagnostic>) {
    let mut ids = BTreeSet::new();
    for pattern in &note.policy_patterns {
        let Some(id) = pattern.id.as_deref() else {
            diagnostics.push(error(
                "missing-policy-pattern-id",
                &note.rel_path,
                Some(pattern.line),
                "policy pattern must declare an id",
            ));
            continue;
        };

        let id = id.trim();
        if id.is_empty() {
            diagnostics.push(error(
                "empty-policy-pattern",
                &note.rel_path,
                Some(pattern.line),
                "policy pattern id may not be empty",
            ));
            continue;
        }

        if !ids.insert(id.to_string()) {
            diagnostics.push(error(
                "duplicate-policy-pattern",
                &note.rel_path,
                Some(pattern.line),
                format!("policy pattern id `{id}` is declared more than once"),
            ));
        }

        if !pattern.has_inline_definition() {
            diagnostics.push(error(
                "missing-policy-pattern-definition",
                &note.rel_path,
                Some(pattern.line),
                format!("policy pattern `{id}` must declare language and pattern or rule"),
            ));
        } else {
            validate_inline_policy_pattern(note, pattern, id, diagnostics);
        }
    }
}

fn validate_inline_policy_pattern(
    note: &Note,
    pattern: &PolicyPattern,
    id: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(language) = pattern
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        diagnostics.push(error(
            "missing-policy-pattern-language",
            &note.rel_path,
            Some(pattern.line),
            format!("inline policy pattern `{id}` must declare a language"),
        ));
        return;
    };

    match (pattern.pattern.as_deref(), pattern.rule.as_deref()) {
        (Some(_), Some(_)) => diagnostics.push(error(
            "ambiguous-policy-pattern-body",
            &note.rel_path,
            Some(pattern.line),
            format!("inline policy pattern `{id}` must declare either pattern or rule, not both"),
        )),
        (None, None) => diagnostics.push(error(
            "missing-policy-pattern-body",
            &note.rel_path,
            Some(pattern.line),
            format!("inline policy pattern `{id}` must declare pattern or rule"),
        )),
        (Some(pattern_body), None) => {
            if let Err(err) = crate::structural::validate_source(
                crate::structural::PatternSource::Pattern(pattern_body),
                language,
            ) {
                diagnostics.push(error(
                    "invalid-policy-pattern",
                    &note.rel_path,
                    Some(pattern.line),
                    format!("inline policy pattern `{id}` does not compile: {err}"),
                ));
            }
        }
        (None, Some(rule_body)) => {
            if let Err(err) = crate::structural::validate_source(
                crate::structural::PatternSource::Rule(rule_body),
                language,
            ) {
                diagnostics.push(error(
                    "invalid-policy-pattern",
                    &note.rel_path,
                    Some(pattern.line),
                    format!("inline policy rule `{id}` does not compile: {err}"),
                ));
            }
        }
    }
}

fn validate_targets(vault: &Vault, note: &Note, diagnostics: &mut Vec<Diagnostic>) {
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

    for pattern in &note.target_pattern_refs {
        if !vault.patterns().contains(&pattern.id) {
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

    for scope in &note.targets_scope {
        if !vault.source_glob_has_match(scope) {
            diagnostics.push(warning(
                "empty-target-scope",
                &note.rel_path,
                None,
                format!("target scope `{scope}` matches no source files"),
            ));
        }
    }

    for governs in vault.effective_governs(note) {
        if !vault.source_glob_has_match(&governs) {
            diagnostics.push(error(
                "unresolved-governs",
                &note.rel_path,
                None,
                format!("governs glob `{}` matches no source files", governs),
            ));
        }
    }
}

fn validate_c4_artifacts(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    for artifact in &vault.c4_artifacts {
        for artifact_diagnostic in &artifact.diagnostics {
            diagnostics.push(error(
                artifact_diagnostic.code,
                &artifact.rel_path,
                artifact_diagnostic.line,
                artifact_diagnostic.message.clone(),
            ));
        }
        for directive in artifact
            .directives
            .iter()
            .filter(|directive| directive.key == "generated")
        {
            if let Some(value) = directive.value.as_deref()
                && !matches!(value, "true" | "false")
            {
                diagnostics.push(error(
                    "invalid-c4-generated",
                    &artifact.rel_path,
                    Some(directive.line),
                    "criv:generated must be `true` or `false` when a value is provided",
                ));
            }
        }

        if artifact.format == Some(C4ArtifactFormat::Dot) {
            if artifact.level.is_some_and(|level| level.as_str() != "code") {
                diagnostics.push(error(
                    "invalid-c4-level",
                    &artifact.rel_path,
                    None,
                    "DOT .c4 artifacts currently support only filename-derived Code level",
                ));
            }
            if let Err(message) = validate_dot_shape(&artifact.path) {
                diagnostics.push(error("invalid-c4-dot", &artifact.rel_path, None, message));
            }
        }

        validate_c4_diagrams(vault, &artifact.rel_path, &artifact.diagrams, diagnostics);
    }
}

fn validate_c4_interface_drift(
    vault: &Vault,
    previous_interface_hashes: Option<&BTreeMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(previous_interface_hashes) = previous_interface_hashes else {
        return;
    };
    for record in state::c4_interface_hash_records(vault) {
        let Some(previous_hash) = previous_interface_hashes.get(&record.id) else {
            continue;
        };
        if previous_hash != &record.hash {
            diagnostics.push(warning(
                "c4-interface-drift",
                &record.path,
                Some(record.line),
                format!(
                    "C4 source `{}` interface changed since the previous state; review the diagram",
                    record.target
                ),
            ));
        }
    }
}

fn validate_dot_shape(path: &Path) -> std::result::Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read DOT .c4 artifact: {err}"))?;
    if !contents.contains('{') || !contents.contains('}') {
        return Err("DOT .c4 artifact must contain a graph body enclosed in braces".into());
    }
    Ok(())
}

fn validate_c4_diagrams(
    vault: &Vault,
    path: &str,
    diagrams: &[crate::c4::C4Diagram],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for diagram in diagrams {
        for (line, source) in &diagram.invalid_source_placements {
            diagnostics.push(error(
                "invalid-c4-source-placement",
                path,
                Some(*line),
                format!("`criv:source {source}` must immediately follow a C4 architecture element"),
            ));
        }

        for (alias, line) in diagram.duplicate_aliases() {
            diagnostics.push(error(
                "duplicate-c4-alias",
                path,
                Some(line),
                format!("C4 element alias `{alias}` is declared more than once"),
            ));
        }

        for (element, line) in diagram.duplicate_sources() {
            diagnostics.push(error(
                "duplicate-c4-source",
                path,
                Some(line),
                format!(
                    "C4 element `{}` has more than one `criv:source` annotation",
                    element.alias
                ),
            ));
        }

        for relationship in diagram.unresolved_relationships() {
            diagnostics.push(error(
                "unresolved-c4-relationship",
                path,
                Some(relationship.line),
                format!(
                    "C4 relationship `{}` -> `{}` references an unknown element alias",
                    relationship.from, relationship.to
                ),
            ));
        }

        for relationship in &diagram.relationships {
            if relationship.label.is_none() {
                diagnostics.push(warning(
                    "missing-c4-relationship-label",
                    path,
                    Some(relationship.line),
                    format!(
                        "C4 relationship `{}` -> `{}` should describe its communication intent",
                        relationship.from, relationship.to
                    ),
                ));
            }
        }

        for element in &diagram.elements {
            if !c4_category_allowed_at_level(diagram.level, element.category) {
                diagnostics.push(error(
                    "invalid-c4-level",
                    path,
                    Some(element.line),
                    format!(
                        "C4 {} diagram cannot contain a {} element `{}`",
                        diagram.level.as_str(),
                        element.category.as_str(),
                        element.alias
                    ),
                ));
            }

            if element.label.is_empty() {
                diagnostics.push(warning(
                    "missing-c4-label",
                    path,
                    Some(element.line),
                    format!("C4 element `{}` should have a label", element.alias),
                ));
            }

            if element.description.is_none() {
                diagnostics.push(warning(
                    "missing-c4-description",
                    path,
                    Some(element.line),
                    format!(
                        "C4 element `{}` should describe its responsibility",
                        element.alias
                    ),
                ));
            }

            if matches!(
                element.category,
                C4ElementCategory::Container | C4ElementCategory::Component
            ) && element.technology.is_none()
            {
                diagnostics.push(warning(
                    "missing-c4-technology",
                    path,
                    Some(element.line),
                    format!(
                        "C4 {} element `{}` should include a technology",
                        element.category.as_str(),
                        element.alias
                    ),
                ));
            }

            let Some(source) = &element.source else {
                continue;
            };
            match vault.resolve_source_target(source) {
                SourceTargetResolution::Resolved { .. } => warn_legacy_source_target(
                    vault,
                    diagnostics,
                    path,
                    Some(element.line),
                    source,
                    "C4 source",
                ),
                SourceTargetResolution::MissingFile => diagnostics.push(error(
                    "unresolved-c4-target",
                    path,
                    Some(element.line),
                    format!(
                        "C4 element `{}` source `{source}` does not resolve to a source file",
                        element.alias
                    ),
                )),
                SourceTargetResolution::MissingFragment { path: resolved_path } => diagnostics.push(error(
                    "unresolved-c4-target",
                    path,
                    Some(element.line),
                    format!(
                        "C4 element `{}` source `{source}` resolves to `{path}` but does not resolve to a source symbol",
                        element.alias,
                        path = resolved_path
                    ),
                )),
            }
        }
    }
}

fn c4_category_allowed_at_level(level: C4Level, category: C4ElementCategory) -> bool {
    match level {
        C4Level::Context => matches!(
            category,
            C4ElementCategory::Person | C4ElementCategory::SoftwareSystem
        ),
        C4Level::Container => !matches!(category, C4ElementCategory::Component),
        C4Level::Component => true,
    }
}

fn validate_pattern_collisions(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    let mut declarations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pattern in &vault.config.patterns {
        declarations
            .entry(pattern.clone())
            .or_default()
            .push("criv.toml".into());
    }
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
        for link in &note.wiki_links {
            match vault.resolve_link(&link.target) {
                ResolvedLink::Broken => diagnostics.push(error(
                    "broken-link",
                    &note.rel_path,
                    Some(link.line),
                    format!("wiki-link `[[{}]]` does not resolve", link.raw),
                )),
                ResolvedLink::Source { path, ambiguous } => {
                    let suggestion = vault
                        .canonical_source_target(&link.target)
                        .map(|target| {
                            format!(
                                "; use AST-aware source selector `{target}` for code references"
                            )
                        })
                        .unwrap_or_default();
                    diagnostics.push(warning(
                        "source-wikilink",
                        &note.rel_path,
                        Some(link.line),
                        format!(
                            "wiki-link `[[{}]]` targets source `{path}`; Wikilinks are reserved for document references{suggestion}",
                            link.raw
                        ),
                    ));
                    if ambiguous {
                        diagnostics.push(warning(
                            "ambiguous-source-link",
                            &note.rel_path,
                            Some(link.line),
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
                        diagnostics.push(error(
                            "non-portable-note-link",
                            &note.rel_path,
                            Some(link.line),
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
    let json = serde_json::to_string_pretty(diagnostics)
        .map_err(|err| CrivError::new(format!("failed to serialize check diagnostics: {err}")))?;
    println!("{json}");
    Ok(())
}

fn error(
    code: &'static str,
    path: &str,
    line: Option<usize>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code,
        path: path.into(),
        line,
        message: message.into(),
    }
}

fn warning(
    code: &'static str,
    path: &str,
    line: Option<usize>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        code,
        path: path.into(),
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::WikiLink;

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
    fn valid_c4_source_annotation_passes() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs#fn:run
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "unresolved-c4-target")
        );
    }

    #[test]
    fn missing_c4_source_target_is_reported() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/missing.rs
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "unresolved-c4-target")
        );
    }

    #[test]
    fn duplicate_c4_source_annotation_is_reported() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs
%% criv:source src/lib.rs
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "duplicate-c4-source")
        );
    }

    #[test]
    fn duplicate_c4_alias_is_reported() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
Container(cli, "Other CLI", "Rust", "Duplicates alias")
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "duplicate-c4-alias")
        );
    }

    #[test]
    fn unresolved_c4_relationship_is_reported() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
Rel(cli, plugin, "writes state for")
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "unresolved-c4-relationship")
        );
    }

    #[test]
    fn relationship_to_c4_boundary_is_unresolved() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
System_Boundary(system, "criv") {
    Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
}
Rel(cli, system, "runs inside")
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "unresolved-c4-relationship")
        );
    }

    #[test]
    fn invalid_c4_level_is_reported_for_mixed_abstractions() {
        let context_vault = c4_vault(
            r#"
```mermaid
C4Context
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
```
"#,
        );
        let container_vault = c4_vault(
            r#"
```mermaid
C4Container
Component(parser, "C4 Parser", "Rust", "Parses Mermaid C4 blocks")
```
"#,
        );

        assert!(
            validate(&context_vault)
                .iter()
                .any(|diag| diag.code == "invalid-c4-level")
        );
        assert!(
            validate(&container_vault)
                .iter()
                .any(|diag| diag.code == "invalid-c4-level")
        );
    }

    #[test]
    fn surrounding_context_elements_are_valid_in_lower_level_diagrams() {
        let vault = c4_vault(
            r#"
```mermaid
C4Component
Person(user, "Maintainer", "Runs checks")
System_Ext(github, "GitHub", "Hosts repositories")
Container(cli, "criv CLI", "Rust", "Runs local validation")
Component(parser, "C4 Parser", "Rust", "Parses Mermaid C4 blocks")
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "invalid-c4-level")
        );
    }

    #[test]
    fn invalid_c4_source_placement_is_reported() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
System_Boundary(system, "criv") {
%% criv:source src/main.rs
}
```
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "invalid-c4-source-placement")
        );
    }

    #[test]
    fn missing_c4_metadata_is_reported_as_warnings() {
        let vault = c4_vault(
            r#"
```mermaid
C4Container
Container(cli, "")
Rel(cli, cli)
```
"#,
        );

        let diagnostics = validate(&vault);

        for code in [
            "missing-c4-label",
            "missing-c4-description",
            "missing-c4-technology",
            "missing-c4-relationship-label",
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|diag| diag.code == code && diag.is_warning()),
                "missing warning {code}"
            );
        }
    }

    #[test]
    fn c4_artifact_mermaid_validation_reuses_c4_rules() {
        let vault = c4_artifact_vault(
            "docs/architecture/02-container.c4",
            r#"
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
Container(cli, "Other CLI", "Rust", "Duplicates alias")
Rel(cli, plugin, "writes state for")
"#,
        );

        let diagnostics = validate(&vault);

        for code in ["duplicate-c4-alias", "unresolved-c4-relationship"] {
            assert!(
                diagnostics
                    .iter()
                    .any(|diag| diag.code == code
                        && diag.path == "docs/architecture/02-container.c4"),
                "missing diagnostic {code}"
            );
        }
    }

    #[test]
    fn c4_artifact_source_annotations_are_validated_as_c4_sources() {
        let vault = c4_artifact_vault(
            "docs/architecture/02-container.c4",
            r#"
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs#fn:run
"#,
        );

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().all(|diag| {
            diag.code != "unknown-c4-directive" && diag.code != "unresolved-c4-target"
        }));
    }

    #[test]
    fn c4_artifact_directive_and_level_errors_are_reported() {
        let vault = c4_artifact_vault(
            "docs/architecture/02-container.c4",
            r#"
%% criv:unknown yes
%% criv:format dot
%% criv:generated maybe
C4Context
Person(user, "Maintainer", "Runs checks")
"#,
        );

        let diagnostics = validate(&vault);

        for code in [
            "unknown-c4-directive",
            "mismatched-c4-format",
            "invalid-c4-generated",
            "mismatched-c4-level",
        ] {
            assert!(
                diagnostics.iter().any(|diag| diag.code == code),
                "missing diagnostic {code}"
            );
        }
    }

    #[test]
    fn c4_artifact_dot_code_file_validates_structurally() {
        let vault = c4_artifact_vault(
            "docs/architecture/04-code.c4",
            r#"
// criv:generated true
digraph criv_code {
  "src/main.rs#fn:run" -> "src/lib.rs#fn:helper";
}
"#,
        );

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.path != "docs/architecture/04-code.c4")
        );
    }

    #[test]
    fn c4_artifact_dot_requires_code_level_and_graph_body() {
        let vault = c4_artifact_vault(
            "docs/architecture/01-context.c4",
            r#"
digraph criv_code
"#,
        );

        let diagnostics = validate(&vault);

        for code in ["invalid-c4-level", "invalid-c4-dot"] {
            assert!(
                diagnostics.iter().any(|diag| diag.code == code),
                "missing diagnostic {code}"
            );
        }
    }

    #[test]
    fn c4_interface_drift_ignores_body_only_changes() {
        let root = c4_interface_drift_fixture(
            r#"
pub fn run(input: String) -> usize {
  input.len()
}
"#,
        );
        let previous_vault = Vault::load(&root).unwrap();
        let previous_state = State::build(&root, &previous_vault).unwrap();
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

        let vault = Vault::load(&root).unwrap();
        let diagnostics = validate_with_previous_state(&vault, Some(&previous_state));

        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "c4-interface-drift")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn c4_interface_drift_warns_on_signature_changes() {
        let root = c4_interface_drift_fixture(
            r#"
pub fn run(input: String) -> usize {
  input.len()
}
"#,
        );
        let previous_vault = Vault::load(&root).unwrap();
        let previous_state = State::build(&root, &previous_vault).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            r#"
pub fn run(input: String, fallback: usize) -> usize {
  fallback
}
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let diagnostics = validate_with_previous_state(&vault, Some(&previous_state));

        assert!(diagnostics.iter().any(|diag| {
            diag.code == "c4-interface-drift"
                && diag.path == "docs/architecture/02-container.c4"
                && diag.is_warning()
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rumdl_fixes_markdown_content_in_process() {
        let path = unique_temp_file("criv-rumdl-fix", "README.md");
        let mut contents = "# Title\n\n\nBody\n".to_string();
        fs::write(&path, &contents).unwrap();
        let config = RumdlConfig::default();
        let mut diagnostics = Vec::new();

        apply_markdown_fixes(&path, "README.md", &mut contents, &config, &mut diagnostics).unwrap();

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
            c4_diagrams: Vec::new(),
            frontmatter_error: None,
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
        }
    }

    fn test_vault(notes: Vec<Note>) -> Vault {
        Vault::from_parts_for_test(notes)
    }

    fn c4_vault(diagram: &str) -> Vault {
        let root = unique_temp_dir("criv-c4-check");
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
        fs::write(root.join("src/lib.rs"), "fn helper() {}\n").unwrap();
        fs::write(
            root.join("docs/c4.md"),
            format!(
                r#"---
id: c4
kind: doc
title: C4
---

# C4
{diagram}
"#
            ),
        )
        .unwrap();
        Vault::load(&root).unwrap()
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

    fn c4_artifact_vault(path: &str, contents: &str) -> Vault {
        let root = unique_temp_dir("criv-c4-artifact-check");
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
        fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        fs::write(root.join("src/lib.rs"), "fn helper() {}\n").unwrap();
        fs::write(root.join(path), contents).unwrap();
        Vault::load(&root).unwrap()
    }

    fn c4_interface_drift_fixture(source: &str) -> std::path::PathBuf {
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
        fs::write(
            root.join("docs/architecture/02-container.c4"),
            r#"
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/lib.rs#fn:run
"#,
        )
        .unwrap();
        root
    }
}

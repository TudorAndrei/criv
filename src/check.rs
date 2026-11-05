mod document;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use std::fs;

use serde::Serialize;
use usage_rs::{Args as UsageArgs, ValueEnum};

use crate::diagnostic::{self, Diagnostic, DiagnosticCode, LspRange, Severity, SourceLocation};
#[cfg(test)]
use crate::git::ChangedEntry;
use crate::git::{ChangeStatus, ChangedSet};
use crate::identity::{adr_filename, is_adr_id, parse_adr_id, parse_adr_stem, pattern_id, slug};
use crate::markdown::LinkTarget;
use crate::policy_scan::PolicyScanPlan;
use crate::repository::RepositoryFiles;
use crate::vault::{Note, NoteKind, ResolvedLink, SourceTargetResolution, Vault};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Github,
}

#[derive(Debug, UsageArgs)]
pub struct CheckOptions {
    #[usage(long, value_enum, default = "text")]
    format: Format,
    #[usage(long)]
    filter: Option<String>,
    /// Validate safely scoped facts for the staged Git transaction.
    #[usage(long)]
    changed: bool,
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    severity: Severity,
    code: DiagnosticCode,
    path: &'a str,
    line: Option<usize>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<LspRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<&'static str>,
}

impl<'a> JsonDiagnostic<'a> {
    fn new(diagnostic: &'a Diagnostic) -> Self {
        Self {
            severity: diagnostic.severity(),
            code: diagnostic.code(),
            path: diagnostic.path(),
            line: diagnostic.line(),
            message: diagnostic.message(),
            range: diagnostic.location().and_then(SourceLocation::lsp_range),
            fix: diagnostic.code().fix(),
        }
    }
}

pub fn run(root: &Path, options: &CheckOptions) -> Result<()> {
    let files = RepositoryFiles::open_vault(root)?;
    let mut diagnostics = if options.changed {
        validate_changed(&files)?
    } else {
        validate_all_from(&files)?
    };

    if let Some(filter) = &options.filter {
        diagnostics.retain(|diag| {
            diag.path().contains(filter)
                || diag.message().contains(filter)
                || diag.code().as_str().contains(filter)
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
        if options.format == Format::Text {
            println!("next: {}", next_command(&diagnostics));
        }
        return Err(CrivError::coded(
            DiagnosticCode::CheckFailed,
            "check failed",
        ));
    }

    Ok(())
}

pub fn validate_all_from(files: &RepositoryFiles) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let vault = Vault::load_from(files)?;
    let policy_plan = PolicyScanPlan::new(&vault);
    diagnostics.extend(validate_vault(&vault, &policy_plan));
    diagnostics.extend(policy_plan.scan(&vault, None)?);
    Ok(diagnostics)
}

fn validate_changed(files: &RepositoryFiles) -> Result<Vec<Diagnostic>> {
    let root = files.root();
    let changes = crate::git::staged_changes(root)?;
    if changes.entries.is_empty() {
        return Ok(Vec::new());
    }

    let config = crate::config::Config::load_from(files)?;
    if changed_scope_requires_full_check(&changes, &config.adr_prefix()) {
        return validate_all_from(files);
    }

    let changed_paths = changes
        .affected_paths()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut diagnostics = Vec::new();
    let vault = Vault::load_from(files)?;
    let policy_plan = PolicyScanPlan::new(&vault);
    diagnostics.extend(validate_changed_vault(&vault, &policy_plan, &changed_paths));
    diagnostics.extend(policy_plan.scan(&vault, Some(&changed_paths))?);
    diagnostic::sort(&mut diagnostics);
    Ok(diagnostics)
}

const SCOPE_INVALIDATING_CONFIG_FILES: [&str; 1] = ["criv.toml"];

fn changed_scope_requires_full_check(changes: &ChangedSet, adr_prefix: &str) -> bool {
    changes.entries.iter().any(|entry| {
        matches!(
            entry.status,
            ChangeStatus::Deleted | ChangeStatus::Renamed | ChangeStatus::Other
        ) || [
            &entry.path,
            entry.previous_path.as_ref().unwrap_or(&entry.path),
        ]
        .into_iter()
        .any(|path| {
            SCOPE_INVALIDATING_CONFIG_FILES.contains(&path.as_str()) || path.starts_with(adr_prefix)
        })
    })
}

#[cfg(test)]
fn validate(vault: &Vault) -> Vec<Diagnostic> {
    let policy_plan = PolicyScanPlan::new(vault);
    validate_vault(vault, &policy_plan)
}

pub fn validate_vault(vault: &Vault, policy_plan: &PolicyScanPlan) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_notes(vault, &mut diagnostics);
    diagnostics.extend_from_slice(policy_plan.definition_diagnostics());
    validate_pattern_collisions(vault, &mut diagnostics);
    validate_links(vault, &mut diagnostics);
    validate_supersession(vault, &mut diagnostics);
    diagnostic::sort(&mut diagnostics);
    diagnostics
}

fn validate_changed_vault(
    vault: &Vault,
    policy_plan: &PolicyScanPlan,
    changed_paths: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for note in vault
        .notes
        .iter()
        .filter(|note| changed_paths.contains(&note.rel_path))
    {
        validate_note_local(vault, note, &mut diagnostics);
        validate_note_links(vault, note, &mut diagnostics);
    }
    diagnostics.extend(
        policy_plan
            .definition_diagnostics()
            .iter()
            .filter(|diagnostic| changed_paths.contains(diagnostic.path()))
            .cloned(),
    );
    diagnostic::sort(&mut diagnostics);
    diagnostics
}

fn validate_notes(vault: &Vault, diagnostics: &mut Vec<Diagnostic>) {
    let mut ids: BTreeMap<&str, Vec<&Note>> = BTreeMap::new();
    for note in &vault.notes {
        validate_note_local(vault, note, diagnostics);
        if let Some(id) = &note.id {
            ids.entry(id).or_default().push(note);
        }
    }

    for (id, notes) in ids {
        if notes.len() > 1 {
            for note in notes {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::DuplicateId,
                    &note.rel_path,
                    None,
                    format!("duplicate note id `{id}`"),
                ));
            }
        }
    }
}

fn validate_note_local(vault: &Vault, note: &Note, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(err) = &note.frontmatter_error {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidFrontmatter,
                &note.rel_path,
                None,
                err.clone(),
            )
            .at(note.frontmatter_error_location.clone()),
        );
    }

    if note.id.as_deref().is_none_or(|id| id.trim().is_empty()) {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::MissingId,
            &note.rel_path,
            None,
            "note is missing required frontmatter `id`",
        ));
    }

    match note.kind {
        NoteKind::Doc | NoteKind::Decision => {}
        NoteKind::Unknown => diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidKind,
            &note.rel_path,
            None,
            "note frontmatter `kind` must be `doc` or `decision`",
        )),
    }

    if note.kind == NoteKind::Decision {
        validate_decision_note(vault, note, diagnostics);
    } else if vault.config.is_adr_path(&note.rel_path) {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::AdrDirNonDecision,
            &note.rel_path,
            None,
            "ADR directory may contain only `kind: decision` notes plus README.md",
        ));
    }

    document::validate(note, vault.is_historical_decision(note), diagnostics);
    validate_targets(vault, note, diagnostics);
}

fn validate_decision_note(vault: &Vault, note: &Note, diagnostics: &mut Vec<Diagnostic>) {
    let adr_id = note.id.as_deref().filter(|id| is_adr_id(id));
    if adr_id.is_none() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidAdrId,
            &note.rel_path,
            None,
            "decision id must match ADR-NNNN",
        ));
    }

    if !note.rel_path.starts_with(&vault.config.adr_prefix()) {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DecisionLocation,
            &note.rel_path,
            None,
            format!(
                "decision notes must live under {}/{}",
                vault.config.docs_dir, vault.config.adr_dir
            ),
        ));
    }

    let title_slug = note.title.as_deref().map(slug).unwrap_or_default();
    if let Some(number) = adr_id.and_then(parse_adr_id)
        && !title_slug.is_empty()
        && !note
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(parse_adr_stem)
            .is_some_and(|(stem_number, stem_slug)| stem_number == number && stem_slug.is_some())
    {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::AdrFilename,
            &note.rel_path,
            None,
            format!(
                "ADR filename should follow NNNN-kebab-case-title.md, as in `{}`",
                adr_filename(number, &title_slug)
            ),
        ));
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
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnresolvedTarget,
                        &note.rel_path,
                        None,
                        format!("target symbol `{target}` does not resolve to a source file"),
                    ));
                }
                SourceTargetResolution::MissingFragment { path } => {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnresolvedTarget,
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
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnresolvedPattern,
                &note.rel_path,
                Some(pattern.line),
                format!("pattern reference `{}` does not resolve", pattern.id),
            ));
        }
    }

    let mut doc_local_patterns = BTreeSet::new();
    for pattern in &note.target_pattern_ids {
        if !doc_local_patterns.insert(pattern) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::DuplicateDocPattern,
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
                diagnostics.push(Diagnostic::warning(
                    DiagnosticCode::EmptyTargetScope,
                    &note.rel_path,
                    None,
                    format!("target scope `{scope}` matches no source files"),
                ));
            }
        }

        for governs in unresolved_governs(vault, note) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnresolvedGoverns,
                &note.rel_path,
                None,
                format!("governs glob `{governs}` matches no source files"),
            ));
        }
    }
}

fn unresolved_governs(vault: &Vault, note: &Note) -> Vec<String> {
    let governs = Vault::effective_governs(note);
    governs
        .iter()
        .zip(vault.source_globs_have_matches(&governs))
        .filter(|(_, has_match)| !has_match)
        .map(|(governs, _)| governs.clone())
        .collect()
}

pub fn publication_blocking_diagnostics(vault: &Vault) -> Vec<Diagnostic> {
    let mut diagnostics = vault
        .notes
        .iter()
        .filter(|note| vault.is_effective_decision(note))
        .flat_map(|note| {
            unresolved_governs(vault, note).into_iter().map(|governs| {
                Diagnostic::error(
                    DiagnosticCode::UnresolvedGoverns,
                    &note.rel_path,
                    None,
                    format!("governs glob `{governs}` matches no source files"),
                )
            })
        })
        .collect::<Vec<_>>();
    diagnostic::sort(&mut diagnostics);
    diagnostics
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
                if let Some(local_id) = pattern.id.as_deref() {
                    declarations
                        .entry(pattern_id(id, local_id))
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
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::DuplicatePatternId,
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
            ResolvedLink::Broken => diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::BrokenLink,
                    &note.rel_path,
                    Some(link.line),
                    format!("wiki-link `[[{}]]` does not resolve", link.raw),
                )
                .at(link.location.clone()),
            ),
            ResolvedLink::Source { path, ambiguous } => {
                let suggestion = vault
                    .canonical_source_target_for_path(&link.target, &path)
                    .map(|target| {
                        format!("; use AST-aware source selector `{target}` for code references")
                    })
                    .unwrap_or_default();
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::SourceWikilink,
                        &note.rel_path,
                        Some(link.line),
                        format!(
                            "wiki-link `[[{}]]` targets source `{path}`; Wikilinks are reserved for document references{suggestion}",
                            link.raw
                        ),
                    )
                    .at(link.location.clone()),
                );
                if ambiguous {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::AmbiguousSourceLink,
                            &note.rel_path,
                            Some(link.line),
                            format!(
                                "wiki-link `[[{}]]` resolves ambiguously; first match is `{path}`",
                                link.raw
                            ),
                        )
                        .at(link.location.clone()),
                    );
                }
            }
            ResolvedLink::Pattern { .. } => {}
            ResolvedLink::Note { .. } => {
                if !vault.is_file_backed_note_target(&link.target) {
                    let suggestion = vault
                        .portable_note_target(&link.target)
                        .map(|target| format!("; use `[[{target}]]` instead"))
                        .unwrap_or_default();
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::NonPortableNoteLink,
                            &note.rel_path,
                            Some(link.line),
                            format!(
                                "wiki-link `[[{}]]` resolves through note metadata but does not target an existing markdown file{suggestion}",
                                link.raw
                            ),
                        )
                        .at(link.location.clone()),
                    );
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
    let written = LinkTarget::parse(target);
    if written.is_source() || written.source_body() != canonical {
        diagnostics.push(Diagnostic::warning(
            DiagnosticCode::LegacySourceTarget,
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
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownSupersedes,
                    &note.rel_path,
                    None,
                    format!("supersedes references unknown decision `{old_id}`"),
                ));
            }
        }

        for new_id in &note.superseded_by {
            match decisions.get(new_id.as_str()) {
                None => diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownSupersededBy,
                    &note.rel_path,
                    None,
                    format!("superseded_by references unknown decision `{new_id}`"),
                )),
                Some(new_note) => {
                    if !new_note.supersedes.iter().any(|value| value == id) {
                        diagnostics.push(Diagnostic::error(
                            DiagnosticCode::InconsistentSupersession,
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
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::SupersessionCycle,
            decisions
                .get(cycle.first().map_or("", String::as_str))
                .map_or("docs", |note| note.rel_path.as_str()),
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
        let mut cycle = stack.get(position..).unwrap_or_default().to_vec();
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

fn next_command(diagnostics: &[Diagnostic]) -> &'static str {
    if diagnostics
        .iter()
        .any(|diag| diag.is_error() && diag.code() == DiagnosticCode::UnresolvedTarget)
    {
        return "criv watch --once";
    }
    "apply the fixes above, then run `criv check`"
}

fn print_text(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        println!("criv check: ok");
        return;
    }

    for diag in diagnostics {
        println!(
            "{}[{}] {}",
            diag.severity().as_str(),
            diag.code(),
            diag.describe()
        );
        if let Some(fix) = diag.code().fix() {
            println!("  fix: {fix}");
        }
    }
}

fn print_json(diagnostics: &[Diagnostic]) -> Result<()> {
    let diagnostics = diagnostics
        .iter()
        .map(JsonDiagnostic::new)
        .collect::<Vec<_>>();
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
    let location = diag
        .location()
        .and_then(SourceLocation::github_location)
        .map_or_else(
            || {
                diag.line()
                    .map(|line| format!(",line={line}"))
                    .unwrap_or_default()
            },
            |location| match (location.column, location.end_column) {
                (Some(column), Some(end_column)) => format!(
                    ",line={},col={column},endLine={},endColumn={end_column}",
                    location.line, location.end_line
                ),
                _ => format!(",line={},endLine={}", location.line, location.end_line),
            },
        );
    format!(
        "::{} file={}{},title={}::{}",
        diag.severity().as_str(),
        escape_github_property(diag.path()),
        location,
        escape_github_property(&format!("criv {}", diag.code())),
        escape_github_message(diag.message())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::WikiLink;

    #[test]
    fn next_command_names_the_repair_criv_can_run() {
        let stale = vec![Diagnostic::error(
            DiagnosticCode::UnresolvedTarget,
            "docs/a.md",
            None,
            "stale",
        )];
        let other = vec![Diagnostic::error(
            DiagnosticCode::BrokenLink,
            "docs/a.md",
            None,
            "missing",
        )];

        assert_eq!(next_command(&stale), "criv watch --once");
        assert!(next_command(&other).contains("criv check"));
    }

    #[test]
    fn json_diagnostics_carry_the_repair() {
        let diagnostic =
            Diagnostic::error(DiagnosticCode::DocumentFormat, "docs/a.md", None, "bad");
        let json = serde_json::to_value(JsonDiagnostic::new(&diagnostic)).expect("serialize");

        assert_eq!(json["code"], "document-format");
        assert_eq!(json["fix"], DiagnosticCode::DocumentFormat.fix().unwrap());
    }

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
            changed_entry(ChangeStatus::Modified, ".oxfmtrc.json", None),
        ]);

        assert!(!changed_scope_requires_full_check(&changes, "docs/adr/"));
    }

    #[test]
    fn changed_scope_promotes_global_transactions_to_full_check() {
        for entry in [
            changed_entry(ChangeStatus::Deleted, "docs/guide.md", None),
            changed_entry(ChangeStatus::Renamed, "docs/new.md", Some("docs/old.md")),
            changed_entry(ChangeStatus::Modified, "docs/adr/0067-decision.md", None),
            changed_entry(ChangeStatus::Modified, "criv.toml", None),
        ] {
            assert!(changed_scope_requires_full_check(
                &changed_set_fixture(vec![entry]),
                "docs/adr/"
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
        let diag = Diagnostic::error(
            DiagnosticCode::BrokenLink,
            "docs/a,b:guide.md",
            Some(7),
            "bad % link\r\ntry again",
        );

        assert_eq!(
            github_annotation(&diag),
            "::error file=docs/a%2Cb%3Aguide.md,line=7,title=criv broken-link::bad %25 link%0D%0Atry again"
        );
    }

    #[test]
    fn github_annotation_omits_missing_line() {
        let diag = Diagnostic::warning(
            DiagnosticCode::MissingId,
            "docs/note.md",
            None,
            "missing id",
        );

        assert_eq!(
            github_annotation(&diag),
            "::warning file=docs/note.md,title=criv missing-id::missing id"
        );
        let json = serde_json::to_value(JsonDiagnostic::new(&diag)).unwrap();
        assert!(json["line"].is_null());
        assert!(json.get("range").is_none());
    }

    #[test]
    fn exact_diagnostics_serialize_lsp_ranges_and_github_columns() {
        let source: Arc<str> = Arc::from("é😀bad\n");
        let location = SourceLocation::new(source, 2..6);
        let diagnostic = Diagnostic::error(
            DiagnosticCode::DocumentFormat,
            "docs/unicode.md",
            Some(1),
            "bad emoji",
        )
        .at(location);

        let json = serde_json::to_value(JsonDiagnostic::new(&diagnostic)).unwrap();
        assert_eq!(
            json["range"],
            serde_json::json!({
                "start": { "line": 0, "character": 1 },
                "end": { "line": 0, "character": 3 }
            })
        );
        assert_eq!(
            github_annotation(&diagnostic),
            "::error file=docs/unicode.md,line=1,col=2,endLine=1,endColumn=2,title=criv document-format::bad emoji"
        );
    }

    #[test]
    fn invalid_exact_locations_keep_the_line_only_shape() {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::DocumentFormat,
            "docs/invalid.md",
            Some(1),
            "invalid range",
        )
        .at(SourceLocation::new(Arc::from("short\n"), 20..21));

        let json = serde_json::to_value(JsonDiagnostic::new(&diagnostic)).unwrap();
        assert!(json.get("range").is_none());
        assert_eq!(
            github_annotation(&diagnostic),
            "::error file=docs/invalid.md,line=1,title=criv document-format::invalid range"
        );
    }

    #[test]
    fn multiline_github_annotations_use_lines_without_unsupported_columns() {
        let source: Arc<str> = Arc::from("first\nsecond\n");
        let location = SourceLocation::new(source, 2..9).unwrap();
        let diagnostic = Diagnostic::error(
            DiagnosticCode::BrokenLink,
            "docs/multi.md",
            None,
            "crosses lines",
        )
        .at(Some(location));

        assert_eq!(
            github_annotation(&diagnostic),
            "::error file=docs/multi.md,line=1,endLine=2,title=criv broken-link::crosses lines"
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
                .all(|diag| diag.code() != DiagnosticCode::InconsistentSupersession)
        );
        old.superseded_by.push("ADR-0003".into());
        let vault = test_vault(vec![old]);
        assert!(
            validate(&vault)
                .iter()
                .any(|diag| diag.code() == DiagnosticCode::UnknownSupersededBy)
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
                .any(|diag| diag.code() == DiagnosticCode::NonPortableNoteLink)
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
                .all(|diag| diag.code() != DiagnosticCode::NonPortableNoteLink)
        );
    }

    #[test]
    fn source_wikilinks_are_reported_as_document_portability_issues() {
        let vault = source_note_vault("[[src/main.rs#fn:run]]", &[]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| {
            diag.code() == DiagnosticCode::SourceWikilink
                && diag
                    .message()
                    .contains("AST-aware source selector `src/main.rs#fn:run`")
        }));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code() != DiagnosticCode::BrokenLink)
        );
    }

    #[test]
    fn adr_0033_source_wikilinks_remain_compatible_but_warn() {
        let vault = source_note_vault("[[source:src/main.rs#run]]", &[]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| {
            diag.code() == DiagnosticCode::SourceWikilink
                && diag
                    .message()
                    .contains("AST-aware source selector `src/main.rs#fn:run`")
        }));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code() != DiagnosticCode::BrokenLink)
        );
    }

    #[test]
    fn typed_source_wikilinks_with_missing_fragments_remain_broken() {
        let vault = source_note_vault("[[source:src/main.rs#missing]]", &[]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code() == DiagnosticCode::BrokenLink)
        );
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code() != DiagnosticCode::SourceWikilink)
        );
    }

    #[test]
    fn ambiguous_legacy_source_wikilinks_retain_both_diagnostics() {
        let vault = source_note_vault("[[shared.rs]]", &[]);

        let diagnostics = validate(&vault);

        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code() == DiagnosticCode::SourceWikilink),
            "{diagnostics:#?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code() == DiagnosticCode::AmbiguousSourceLink
                    && diag.message().contains("first match is `")),
            "{diagnostics:#?}"
        );
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code() != DiagnosticCode::BrokenLink)
        );
    }

    #[test]
    fn legacy_target_symbols_suggest_ast_aware_selectors() {
        let vault = source_note_vault("", &["src/main.rs#run"]);

        let diagnostics = validate(&vault);

        assert!(diagnostics.iter().any(|diag| {
            diag.code() == DiagnosticCode::LegacySourceTarget
                && diag
                    .message()
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
                .all(|diag| diag.code() != DiagnosticCode::LegacySourceTarget)
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
                .all(|diag| diag.code() != DiagnosticCode::UnresolvedGoverns)
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
            diagnostic.code() != DiagnosticCode::UnresolvedGoverns
                && diagnostic.code() != DiagnosticCode::UnresolvedTarget
                && diagnostic.code() != DiagnosticCode::EmptyTargetScope
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
        assert_eq!(blockers[0].code(), DiagnosticCode::UnresolvedGoverns);
        assert_eq!(blockers[0].path(), "docs/adr/0001-old.md");
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
            .find(|diag| diag.code() == DiagnosticCode::BrokenLink)
            .expect("the dangling wiki link must be reported");
        assert_eq!(
            broken.line(),
            Some(expected),
            "the diagnostic must point at the real file line, not a body-relative one"
        );

        let _ = fs::remove_dir_all(root);
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
}

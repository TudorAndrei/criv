use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use clap::{Args as ClapArgs, ValueEnum};

use crate::util::{is_adr_id, kebab};
use crate::vault::{Note, NoteKind, ResolvedLink, Vault, source_fragment_path};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
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
    let vault = Vault::load(root)?;
    let mut diagnostics = validate(&vault);
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
        Format::Json => print_json(&diagnostics),
    }

    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(CrivError::new("check failed"));
    }

    Ok(())
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
        let scopes = vault.effective_governs(note);
        for pattern in &note.policy_pattern_ids {
            let pattern_id = format!("{adr_id}/{pattern}");
            let rows =
                crate::structural::find_policy_pattern(root, vault, &pattern_id, pattern, &scopes)?;
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

pub(crate) fn validate(vault: &Vault) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_notes(vault, &mut diagnostics);
    validate_pattern_collisions(vault, &mut diagnostics);
    validate_links(vault, &mut diagnostics);
    validate_supersession(vault, &mut diagnostics);
    diagnostics.sort_by(|a, b| {
        (&a.path, a.line.unwrap_or(0), a.code).cmp(&(&b.path, b.line.unwrap_or(0), b.code))
    });
    diagnostics
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

    if let Some(filename) = note.path.file_name().map(|value| value.to_string_lossy()) {
        if is_adr_id(id) {
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

    for pattern in &note.policy_pattern_ids {
        if pattern.trim().is_empty() {
            diagnostics.push(error(
                "empty-policy-pattern",
                &note.rel_path,
                None,
                "policy pattern id may not be empty",
            ));
        }
    }
}

fn validate_targets(vault: &Vault, note: &Note, diagnostics: &mut Vec<Diagnostic>) {
    for target in &note.targets_symbols {
        let path = source_fragment_path(target);
        if vault.resolve_source_path(path).is_none() {
            diagnostics.push(error(
                "unresolved-target",
                &note.rel_path,
                None,
                format!("target symbol `{target}` does not resolve to a source file"),
            ));
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
            for pattern in &note.policy_pattern_ids {
                declarations
                    .entry(format!("{id}/{pattern}"))
                    .or_default()
                    .push(note.rel_path.clone());
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
                ResolvedLink::Pattern { .. } | ResolvedLink::Note { .. } => {}
            }
        }
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
            match decisions.get(old_id.as_str()) {
                None => diagnostics.push(error(
                    "unknown-supersedes",
                    &note.rel_path,
                    None,
                    format!("supersedes references unknown decision `{old_id}`"),
                )),
                Some(old_note) => {
                    if !old_note.superseded_by.iter().any(|value| value == id) {
                        diagnostics.push(error(
                            "inconsistent-supersession",
                            &note.rel_path,
                            None,
                            format!("`{old_id}` must list `{id}` in superseded_by"),
                        ));
                    }
                }
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
        for next in &note.superseded_by {
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

fn print_json(diagnostics: &[Diagnostic]) {
    println!("[");
    for (index, diag) in diagnostics.iter().enumerate() {
        let comma = if index + 1 == diagnostics.len() {
            ""
        } else {
            ","
        };
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let line = diag
            .line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "null".into());
        println!(
            "  {{\"severity\":\"{}\",\"code\":\"{}\",\"path\":\"{}\",\"line\":{},\"message\":\"{}\"}}{}",
            severity,
            diag.code,
            json_escape(&diag.path),
            line,
            json_escape(&diag.message),
            comma
        );
    }
    println!("]");
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

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_are_detected() {
        let mut a = empty_decision("ADR-0001");
        let mut b = empty_decision("ADR-0002");
        a.superseded_by.push("ADR-0002".into());
        b.superseded_by.push("ADR-0001".into());
        let decisions = BTreeMap::from([("ADR-0001", &a), ("ADR-0002", &b)]);
        assert!(!supersession_cycles(&decisions).is_empty());
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
            policy_pattern_ids: Vec::new(),
            governs: Vec::new(),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            wiki_links: Vec::new(),
            frontmatter_error: None,
        }
    }
}

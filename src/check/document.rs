//! Criv's document structure, independent of Markdown presentation tools.

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::identity::slug;
use crate::vault::{Note, NoteKind};

pub(super) fn validate(note: &Note, historical: bool, diagnostics: &mut Vec<Diagnostic>) {
    if note.frontmatter_error.is_some() {
        return;
    }
    let title = note
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    if title.is_none() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DocumentFormat,
            &note.rel_path,
            None,
            "note requires non-empty frontmatter `title`",
        ));
    }
    if note.kind == NoteKind::Decision
        && !matches!(
            note.status.as_deref(),
            Some("draft" | "accepted" | "rejected" | "superseded")
        )
    {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DocumentFormat,
            &note.rel_path,
            None,
            "decision requires frontmatter `status`: draft, accepted, rejected, or superseded",
        ));
    }
    // Accepted history is immutable. Its active successor carries today's schema.
    if historical {
        return;
    }
    let mut titles = note.headings.iter().filter(|heading| heading.level == 1);
    let heading = titles.next();
    if heading.is_none() || titles.next().is_some() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DocumentFormat,
            &note.rel_path,
            heading.map(|heading| heading.line),
            "note requires exactly one H1 title heading",
        ));
    } else if let (Some(title), Some(heading)) = (title, heading)
        && slug(title) != slug(&heading.text)
    {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DocumentFormat,
            &note.rel_path,
            Some(heading.line),
            "H1 heading must match frontmatter `title` (case and punctuation are ignored)",
        ));
    }
    if note.kind == NoteKind::Decision {
        let required = ["Context", "Decision", "Consequences"];
        let sections = note
            .headings
            .iter()
            .filter(|heading| heading.level == 2 && required.contains(&heading.text.as_str()))
            .map(|heading| heading.text.as_str())
            .collect::<Vec<_>>();
        if sections != required {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::DocumentFormat,
                &note.rel_path,
                None,
                "active ADR requires exactly one H2 Context, Decision, and Consequences section, in that order",
            ));
        }
    }
}

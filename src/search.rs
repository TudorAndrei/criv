use std::path::Path;

use clap::{Args as ClapArgs, ValueEnum};

use crate::source_index::SourceGrepMode;
use crate::structural::{self, PatternSource, StructuralMatch};
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GrepMode {
    Plain,
    Regex,
    Fuzzy,
}

impl From<GrepMode> for SourceGrepMode {
    fn from(value: GrepMode) -> Self {
        match value {
            GrepMode::Plain => Self::Plain,
            GrepMode::Regex => Self::Regex,
            GrepMode::Fuzzy => Self::Fuzzy,
        }
    }
}

#[derive(Debug)]
enum Mode {
    Grep(String),
    Files(String),
    Notes(String),
    Structural(String),
    Rule(String),
    PatternId(String),
}

#[derive(Debug, ClapArgs)]
pub(crate) struct SearchOptions {
    #[arg(long)]
    grep: Option<String>,
    #[arg(long, value_enum, default_value_t = GrepMode::Plain)]
    grep_mode: GrepMode,
    #[arg(long)]
    files: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    rule: Option<String>,
    #[arg(long)]
    pattern_id: Option<String>,
    #[arg(long = "paths")]
    paths: Vec<String>,
    #[arg(long)]
    semantic: bool,
    #[arg(long)]
    lang: Option<String>,
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    pattern: Option<String>,
}

impl SearchOptions {
    fn mode(&self) -> Result<Mode> {
        let mut modes = Vec::new();
        if let Some(value) = &self.grep {
            modes.push(Mode::Grep(value.clone()));
        }
        if let Some(value) = &self.files {
            modes.push(Mode::Files(value.clone()));
        }
        if let Some(value) = &self.notes {
            modes.push(Mode::Notes(value.clone()));
        }
        if let Some(value) = &self.rule {
            modes.push(Mode::Rule(value.clone()));
        }
        if let Some(value) = &self.pattern_id {
            modes.push(Mode::PatternId(value.clone()));
        }
        if let Some(value) = &self.pattern {
            modes.push(Mode::Structural(value.clone()));
        }

        if modes.is_empty() {
            return Err(CrivError::usage(
                "missing search mode; use --grep, --files, --notes, --pattern-id, --rule, or a structural pattern",
            ));
        }
        if modes.len() > 1 {
            return Err(CrivError::usage(
                "only one search mode may be used at a time",
            ));
        }
        Ok(modes.remove(0))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Row {
    pub(crate) path: String,
    pub(crate) line: Option<usize>,
    pub(crate) text: String,
}

pub(crate) fn run(root: &Path, options: SearchOptions) -> Result<()> {
    let vault = Vault::load(root)?;
    let mut paths = options.paths.clone();
    if paths.is_empty()
        && let Some(language) = &options.lang
    {
        paths.push(structural::language_glob(language).to_string());
    }
    let rows = match options.mode()? {
        Mode::Grep(text) => grep(&vault, &text, options.grep_mode.into(), &paths)?,
        Mode::Files(query) => files(&vault, &query)?,
        Mode::Notes(text) => notes(&vault, &text, options.semantic),
        Mode::Structural(pattern) => structural_rows(structural::find(
            root,
            &vault,
            PatternSource::Pattern(&pattern),
            &paths,
            options.lang.as_deref(),
        )?),
        Mode::Rule(rule) => search_rule(root, &vault, &rule, &paths)?,
        Mode::PatternId(pattern_id) => search_pattern_id(root, &vault, &pattern_id, &paths)?,
    };

    print_rows(&rows, options.format);
    Ok(())
}

fn search_pattern_id(
    root: &Path,
    vault: &Vault,
    pattern_id: &str,
    paths: &[String],
) -> Result<Vec<Row>> {
    if !vault.config.pattern_defs.contains_key(pattern_id) {
        return Err(CrivError::new(format!(
            "registered pattern `{pattern_id}` does not resolve"
        )));
    }
    structural::find_pattern_id(root, vault, pattern_id, paths).map(structural_rows)
}

fn search_rule(root: &Path, vault: &Vault, adr_id: &str, paths: &[String]) -> Result<Vec<Row>> {
    let note = vault
        .resolve_note(adr_id)
        .ok_or_else(|| CrivError::new(format!("decision `{adr_id}` does not resolve")))?;
    let default_scopes;
    let scopes = if paths.is_empty() {
        default_scopes = vault.effective_governs(note);
        &default_scopes
    } else {
        paths
    };
    let mut rows = Vec::new();
    for pattern in &note.policy_pattern_ids {
        let pattern_id = format!("{}/{}", note.display_id(), pattern);
        rows.extend(structural_rows(structural::find_policy_pattern(
            root,
            vault,
            &pattern_id,
            pattern,
            scopes,
        )?));
    }
    rows.sort_by(|left, right| (&left.path, left.line).cmp(&(&right.path, right.line)));
    rows.dedup_by(|left, right| left.path == right.path && left.line == right.line);
    Ok(rows)
}

fn grep(vault: &Vault, text: &str, mode: SourceGrepMode, paths: &[String]) -> Result<Vec<Row>> {
    Ok(vault
        .source_index()
        .grep(text, mode, paths)?
        .into_iter()
        .map(|matched| Row {
            path: matched.path,
            line: Some(matched.line),
            text: matched.text,
        })
        .collect())
}

fn files(vault: &Vault, query: &str) -> Result<Vec<Row>> {
    Ok(vault
        .source_index()
        .fuzzy_files(query, 100)?
        .into_iter()
        .map(|hit| Row {
            path: hit.path,
            line: None,
            text: String::new(),
        })
        .collect())
}

fn notes(vault: &Vault, text: &str, semantic: bool) -> Vec<Row> {
    let query_terms = tokenize(text);
    let mut scored = Vec::new();
    for note in &vault.notes {
        if let Some((score, excerpt)) = note_score(note, &query_terms) {
            scored.push((score, note.rel_path.clone(), note_title(note), excerpt));
        }
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    let mut rows = scored
        .into_iter()
        .map(|(_, path, title, excerpt)| Row {
            path,
            line: None,
            text: if excerpt.is_empty() {
                title
            } else {
                format!("{title} - {excerpt}")
            },
        })
        .collect::<Vec<_>>();
    if semantic {
        rows.push(Row {
            path: "semantic".into(),
            line: None,
            text: "semantic embeddings are not enabled in this MVP".into(),
        });
    }
    rows
}

fn note_score(note: &crate::vault::Note, query_terms: &[String]) -> Option<(i32, String)> {
    if query_terms.is_empty() {
        return None;
    }

    let id = note.id.as_deref().unwrap_or_default().to_lowercase();
    let title = note.title.as_deref().unwrap_or_default().to_lowercase();
    let path = note.rel_path.to_lowercase();
    let heading_text = note
        .headings
        .iter()
        .map(|heading| heading.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let body = note.body.to_lowercase();

    let mut score = 0;
    for term in query_terms {
        let mut term_score = 0;
        if id.contains(term) {
            term_score += 150;
        }
        if title.contains(term) {
            term_score += 100;
        }
        if path.contains(term) {
            term_score += 50;
        }
        if heading_text.contains(term) {
            term_score += 35;
        }
        term_score += body.matches(term).count().min(10) as i32 * 10;
        if term_score == 0 {
            return None;
        }
        score += term_score;
    }

    Some((score, excerpt(&note.body, query_terms)))
}

fn note_title(note: &crate::vault::Note) -> String {
    note.title
        .clone()
        .or_else(|| note.id.clone())
        .unwrap_or_else(|| note.rel_path.clone())
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn excerpt(body: &str, query_terms: &[String]) -> String {
    let body_lower = body.to_lowercase();
    let Some(offset) = query_terms
        .iter()
        .filter_map(|term| body_lower.find(term))
        .min()
    else {
        return String::new();
    };
    let start = body[..offset]
        .rfind(|ch: char| ['.', '\n'].contains(&ch))
        .map(|index| index + 1)
        .unwrap_or(0);
    let end = body[offset..]
        .find(|ch: char| ['.', '\n'].contains(&ch))
        .map(|index| offset + index)
        .unwrap_or_else(|| body.len());
    body[start..end].trim().to_string()
}

fn structural_rows(matches: Vec<StructuralMatch>) -> Vec<Row> {
    matches
        .into_iter()
        .map(|matched| Row {
            path: matched.path,
            line: Some(matched.line),
            text: if matched.captures.is_empty() {
                matched.text
            } else {
                format!("{} [{}]", matched.text, capture_summary(&matched.captures))
            },
        })
        .collect()
}

fn capture_summary(captures: &std::collections::BTreeMap<String, String>) -> String {
    captures
        .iter()
        .map(|(name, value)| format!("${name}={}", value.replace('\n', "\\n")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_rows(rows: &[Row], format: Format) {
    match format {
        Format::Text => {
            for row in rows {
                if let Some(line) = row.line {
                    println!("{}:{line}:{}", row.path, row.text);
                } else if row.text.is_empty() {
                    println!("{}", row.path);
                } else {
                    println!("{}: {}", row.path, row.text);
                }
            }
        }
        Format::Json => {
            println!("[");
            for (index, row) in rows.iter().enumerate() {
                let comma = if index + 1 == rows.len() { "" } else { "," };
                let line = row
                    .line
                    .map(|line| line.to_string())
                    .unwrap_or_else(|| "null".into());
                println!(
                    "  {{\"path\":\"{}\",\"line\":{},\"text\":\"{}\"}}{}",
                    json_escape(&row.path),
                    line,
                    json_escape(&row.text),
                    comma
                );
            }
            println!("]");
        }
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
    fn note_search_requires_all_terms_and_ranks_title_matches() {
        let title = crate::vault::Note {
            path: "docs/title.md".into(),
            rel_path: "docs/title.md".into(),
            id: Some("TITLE".into()),
            kind: crate::vault::NoteKind::Doc,
            title: Some("Async runtime".into()),
            status: None,
            body: "Background text".into(),
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
        };
        let body = crate::vault::Note {
            title: Some("Runtime notes".into()),
            body: "This note talks about async work in detail.".into(),
            rel_path: "docs/body.md".into(),
            path: "docs/body.md".into(),
            ..title.clone()
        };

        let title_score = note_score(&title, &tokenize("async")).unwrap().0;
        let body_score = note_score(&body, &tokenize("async")).unwrap().0;
        assert!(title_score > body_score);
        assert!(note_score(&body, &tokenize("async missing")).is_none());
    }
}

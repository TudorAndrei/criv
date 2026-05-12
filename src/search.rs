use std::path::Path;

use clap::{Args as ClapArgs, ValueEnum};

use crate::util::{glob_matches, read_to_string};
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
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
    if let Some(language) = &options.lang {
        paths.push(language_glob(language).to_string());
    }
    let rows = match options.mode()? {
        Mode::Grep(text) => grep(root, &vault, &text, &paths)?,
        Mode::Files(query) => files(&vault, &query),
        Mode::Notes(text) => notes(&vault, &text, options.semantic),
        Mode::Structural(pattern) => grep(root, &vault, &pattern_to_needle(&pattern), &paths)?,
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
    let Some(pattern) = vault.config.pattern_defs.get(pattern_id) else {
        return Err(CrivError::new(format!(
            "registered pattern `{pattern_id}` does not resolve"
        )));
    };
    let Some(pattern) = pattern.lexical_pattern() else {
        return Err(CrivError::new(format!(
            "registered pattern `{pattern_id}` has no searchable pattern body"
        )));
    };
    let mut scoped_paths = paths.to_vec();
    if let Some(language) = &vault.config.pattern_defs[pattern_id].language {
        scoped_paths.push(language_glob(language).to_string());
    }
    grep(root, vault, &pattern_to_needle(pattern), &scoped_paths)
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
        let mut scoped_paths = scopes.to_vec();
        let pattern_body = if let Some(pattern_def) = vault.config.pattern_defs.get(&pattern_id) {
            if let Some(language) = &pattern_def.language {
                scoped_paths.push(language_glob(language).to_string());
            }
            pattern_def.lexical_pattern().unwrap_or(pattern)
        } else {
            pattern
        };
        rows.extend(grep(
            root,
            vault,
            &pattern_to_needle(pattern_body),
            &scoped_paths,
        )?);
    }
    rows.sort_by(|left, right| (&left.path, left.line).cmp(&(&right.path, right.line)));
    rows.dedup_by(|left, right| left.path == right.path && left.line == right.line);
    Ok(rows)
}

fn grep(root: &Path, vault: &Vault, text: &str, paths: &[String]) -> Result<Vec<Row>> {
    let needle = text.to_lowercase();
    let mut rows = Vec::new();
    for source_file in vault.source_files() {
        if !path_allowed(source_file, paths) {
            continue;
        }
        let path = root.join(source_file);
        let contents = read_to_string(&path)?;
        for (line_index, line) in contents.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                rows.push(Row {
                    path: source_file.clone(),
                    line: Some(line_index + 1),
                    text: line.trim().to_string(),
                });
            }
        }
    }
    Ok(rows)
}

pub(crate) fn search_lexical_pattern(
    root: &Path,
    vault: &Vault,
    pattern: &str,
    paths: &[String],
) -> Result<Vec<Row>> {
    grep(root, vault, &pattern_to_needle(pattern), paths)
}

fn files(vault: &Vault, query: &str) -> Vec<Row> {
    let mut scored = vault
        .source_files()
        .iter()
        .filter_map(|path| fuzzy_score(path, query).map(|score| (score, path)))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left_path), (right_score, right_path)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_path.cmp(right_path))
    });
    scored
        .into_iter()
        .map(|(_, path)| Row {
            path: path.clone(),
            line: None,
            text: String::new(),
        })
        .collect()
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

fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| glob_matches(pattern, path))
}

fn pattern_to_needle(pattern: &str) -> String {
    pattern
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ':' || ch == '.'))
        .filter(|part| {
            !part.is_empty()
                && !part.starts_with('$')
                && *part != "pattern"
                && *part != "inside"
                && *part != "kind"
                && *part != "has"
                && *part != "regex"
        })
        .max_by_key(|part| part.len())
        .unwrap_or(pattern)
        .to_string()
}

fn language_glob(language: &str) -> &'static str {
    match language {
        "rust" => "**/*.rs",
        "typescript" => "**/*.ts",
        "javascript" => "**/*.js",
        "python" => "**/*.py",
        "go" => "**/*.go",
        _ => "**",
    }
}

fn fuzzy_score(path: &str, query: &str) -> Option<i32> {
    let path_lower = path.to_lowercase();
    let query_lower = query.to_lowercase();
    if path_lower.contains(&query_lower) {
        return Some(10_000 - path.len() as i32);
    }

    let mut score = 0;
    let mut query_chars = query_lower.chars();
    let mut current = query_chars.next()?;
    for (index, ch) in path_lower.chars().enumerate() {
        if ch == current {
            score += 100 - index.min(80) as i32;
            if let Some(next) = query_chars.next() {
                current = next;
            } else {
                return Some(score);
            }
        }
    }
    None
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
    fn fuzzy_score_accepts_subsequence() {
        assert!(fuzzy_score("src/auth/verify.rs", "svr").is_some());
        assert!(fuzzy_score("src/auth/verify.rs", "zzz").is_none());
    }

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

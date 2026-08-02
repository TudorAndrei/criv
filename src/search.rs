use std::path::Path;

use clap::{Args as ClapArgs, ValueEnum};
use serde::Serialize;

use crate::source_index::SourceGrepMode;
use crate::structural::{self, PathScope, PatternSource, StructuralMatch};
use crate::util::GlobMatcher;
use crate::vault::Vault;
use crate::{CrivError, Result};

const FILE_SEARCH_LIMIT: usize = 100;

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

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct Row {
    path: String,
    line: Option<usize>,
    text: String,
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
        Mode::Files(query) => files(&vault, &query, &paths)?,
        Mode::Notes(text) => notes(root, &vault, &text, options.semantic)?,
        Mode::Structural(pattern) => structural_rows(structural::find(
            root,
            &vault,
            PatternSource::Pattern(&pattern),
            PathScope::from_paths(&paths),
            options.lang.as_deref(),
        )?),
        Mode::Rule(rule) => search_rule(root, &vault, &rule, &paths)?,
        Mode::PatternId(pattern_id) => search_pattern_id(root, &vault, &pattern_id, &paths)?,
    };

    print_rows(&rows, options.format)
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
    structural::find_pattern_id(root, vault, pattern_id, PathScope::from_paths(paths))
        .map(structural_rows)
}

fn search_rule(root: &Path, vault: &Vault, adr_id: &str, paths: &[String]) -> Result<Vec<Row>> {
    let note = vault
        .resolve_note(adr_id)
        .ok_or_else(|| CrivError::new(format!("decision `{adr_id}` does not resolve")))?;
    let default_scopes;
    let scopes = if paths.is_empty() {
        default_scopes = policy_scope_files(vault, &vault.effective_governs(note));
        &default_scopes
    } else {
        paths
    };
    let mut rows = Vec::new();
    for pattern in &note.policy_patterns {
        rows.extend(structural_rows(structural::find_policy_pattern_entry(
            root,
            vault,
            pattern,
            // `search_rule` already substitutes the decision's governed scopes
            // when no `--paths` filter is given, so an empty list here means
            // "nothing in scope" and must keep matching nothing.
            PathScope::Globs(scopes),
        )?));
    }
    rows.sort_by(|left, right| (&left.path, left.line).cmp(&(&right.path, right.line)));
    rows.dedup_by(|left, right| left.path == right.path && left.line == right.line);
    Ok(rows)
}

fn policy_scope_files(vault: &Vault, scopes: &[String]) -> Vec<String> {
    vault
        .source_files_matching_globs(scopes)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
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

fn files(vault: &Vault, query: &str, paths: &[String]) -> Result<Vec<Row>> {
    let matcher = (!paths.is_empty())
        .then(|| GlobMatcher::new(paths))
        .transpose()?;
    let fuzzy_limit = if matcher.is_some() {
        vault.source_index().entries()?.len().max(FILE_SEARCH_LIMIT)
    } else {
        FILE_SEARCH_LIMIT
    };

    Ok(vault
        .source_index()
        .fuzzy_files(query, fuzzy_limit)?
        .into_iter()
        .filter(|hit| {
            matcher
                .as_ref()
                .is_none_or(|matcher| matcher.is_match(&hit.path))
        })
        .take(FILE_SEARCH_LIMIT)
        .map(|hit| Row {
            path: hit.path,
            line: None,
            text: String::new(),
        })
        .collect())
}

fn notes(root: &Path, vault: &Vault, text: &str, semantic: bool) -> Result<Vec<Row>> {
    if semantic {
        if !vault.config.embeddings {
            return Err(CrivError::new(
                "semantic note search requires `index.embeddings = true` in criv.toml",
            ));
        }
        return semantic_notes(root, vault, text);
    }

    Ok(lexical_notes(vault, text))
}

fn lexical_notes(vault: &Vault, text: &str) -> Vec<Row> {
    let query_terms = tokenize(text);
    let mut scored = Vec::new();
    for note in &vault.notes {
        if let Some((score, excerpt)) = note_score(note, &query_terms) {
            scored.push((score, note.rel_path.clone(), note_title(note), excerpt));
        }
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    scored
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
        .collect::<Vec<_>>()
}

#[cfg(feature = "embeddings")]
fn semantic_notes(root: &Path, vault: &Vault, text: &str) -> Result<Vec<Row>> {
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let cache_dir = root.join(".criv").join("embeddings");
    std::fs::create_dir_all(&cache_dir)?;

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(false),
    )
    .map_err(|err| CrivError::new(format!("failed to initialize fastembed: {err}")))?;

    let query = format!("query: {}", text.trim());
    let documents = vault
        .notes
        .iter()
        .map(|note| format!("passage: {}\n{}", note_title(note), note.body))
        .collect::<Vec<_>>();
    if documents.is_empty() {
        return Ok(Vec::new());
    }

    let query_embedding = model
        .embed([query], None)
        .map_err(|err| CrivError::new(format!("failed to embed query: {err}")))?
        .into_iter()
        .next()
        .ok_or_else(|| CrivError::new("fastembed returned no query embedding"))?;
    let document_embeddings = model
        .embed(documents, None)
        .map_err(|err| CrivError::new(format!("failed to embed notes: {err}")))?;

    Ok(semantic_rows(
        &vault.notes,
        &query_embedding,
        &document_embeddings,
    ))
}

#[cfg(not(feature = "embeddings"))]
fn semantic_notes(_root: &Path, _vault: &Vault, _text: &str) -> Result<Vec<Row>> {
    Err(CrivError::new(
        "semantic note search requires building criv with `--features embeddings`",
    ))
}

#[cfg(any(feature = "embeddings", test))]
fn semantic_rows(
    notes: &[crate::vault::Note],
    query_embedding: &[f32],
    document_embeddings: &[Vec<f32>],
) -> Vec<Row> {
    let mut scored = notes
        .iter()
        .zip(document_embeddings)
        .filter_map(|(note, embedding)| {
            cosine_similarity(query_embedding, embedding).map(|score| {
                (
                    score,
                    note.rel_path.clone(),
                    note_title(note),
                    semantic_excerpt(&note.body),
                )
            })
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
    });
    scored
        .into_iter()
        .take(20)
        .map(|(_, path, title, excerpt)| Row {
            path,
            line: None,
            text: if excerpt.is_empty() {
                title
            } else {
                format!("{title} - {excerpt}")
            },
        })
        .collect()
}

#[cfg(any(feature = "embeddings", test))]
fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

#[cfg(any(feature = "embeddings", test))]
fn semantic_excerpt(body: &str) -> String {
    body.split_whitespace()
        .take(28)
        .collect::<Vec<_>>()
        .join(" ")
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
    let Some(lower_offset) = query_terms
        .iter()
        .filter_map(|term| body_lower.find(term))
        .min()
    else {
        return String::new();
    };
    // `to_lowercase` can change byte lengths, so a byte offset into
    // `body_lower` is not valid for `body`.
    let char_index = body_lower[..lower_offset].chars().count();
    let offset = body
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(body.len());
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

fn print_rows(rows: &[Row], format: Format) -> Result<()> {
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
            Ok(())
        }
        Format::Json => {
            let json = serde_json::to_string_pretty(rows)
                .map_err(|err| CrivError::new(format!("failed to serialize search rows: {err}")))?;
            println!("{json}");
            Ok(())
        }
    }
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
            policy_patterns: Vec::new(),
            governs: Vec::new(),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            wiki_links: Vec::new(),
            c4_diagrams: Vec::new(),
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

    #[test]
    fn excerpt_handles_multibyte_lowercase_expansion() {
        let text = excerpt("İİaé. tail", &["a".to_string()]);

        assert!(!text.is_empty());
    }

    #[test]
    fn excerpt_returns_matching_sentence() {
        let text = excerpt(
            "First sentence has setup. Second sentence has needle. Third sentence.",
            &["needle".to_string()],
        );

        assert_eq!(text, "Second sentence has needle");
    }

    #[test]
    fn semantic_rows_rank_by_cosine_similarity() {
        let one = crate::vault::Note {
            path: "docs/one.md".into(),
            rel_path: "docs/one.md".into(),
            id: Some("ONE".into()),
            kind: crate::vault::NoteKind::Doc,
            title: Some("One".into()),
            status: None,
            body: "first body".into(),
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
        };
        let two = crate::vault::Note {
            title: Some("Two".into()),
            rel_path: "docs/two.md".into(),
            path: "docs/two.md".into(),
            body: "second body".into(),
            ..one.clone()
        };

        let rows = semantic_rows(&[one, two], &[1.0, 0.0], &[vec![0.2, 0.8], vec![0.9, 0.1]]);

        assert_eq!(rows[0].path, "docs/two.md");
        assert_eq!(rows[1].path, "docs/one.md");
    }

    #[test]
    fn cosine_similarity_rejects_mismatched_or_zero_vectors() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), None);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), None);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), Some(1.0));
    }
}

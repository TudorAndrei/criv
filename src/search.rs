use std::path::Path;

use crate::util::{glob_matches, read_to_string};
use crate::vault::Vault;
use crate::{Args, CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug)]
pub(crate) struct SearchOptions {
    mode: Option<Mode>,
    paths: Vec<String>,
    semantic: bool,
    format: Format,
}

impl SearchOptions {
    pub(crate) fn parse(mut args: Args) -> Result<Self> {
        let mut options = Self {
            mode: None,
            paths: Vec::new(),
            semantic: false,
            format: Format::Text,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--grep" => options.set_mode(Mode::Grep(args.expect_value("--grep")?))?,
                "--files" => options.set_mode(Mode::Files(args.expect_value("--files")?))?,
                "--notes" => options.set_mode(Mode::Notes(args.expect_value("--notes")?))?,
                "--rule" => options.set_mode(Mode::Rule(args.expect_value("--rule")?))?,
                "--pattern-id" => {
                    options.set_mode(Mode::PatternId(args.expect_value("--pattern-id")?))?
                }
                "--paths" => options.paths.push(args.expect_value("--paths")?),
                "--semantic" => options.semantic = true,
                "--lang" => {
                    let _ = args.expect_value("--lang")?;
                }
                "--format" => {
                    options.format = match args.expect_value("--format")?.as_str() {
                        "text" => Format::Text,
                        "json" => Format::Json,
                        value => {
                            return Err(CrivError::usage(format!(
                                "unsupported search format `{value}`"
                            )));
                        }
                    };
                }
                value if !value.starts_with('-') => {
                    options.set_mode(Mode::Structural(value.to_string()))?;
                }
                other => return Err(CrivError::usage(format!("unknown search option `{other}`"))),
            }
        }

        if options.mode.is_none() {
            return Err(CrivError::usage(
                "missing search mode; use --grep, --files, --notes, --pattern-id, --rule, or a structural pattern",
            ));
        }

        Ok(options)
    }

    fn set_mode(&mut self, mode: Mode) -> Result<()> {
        if self.mode.is_some() {
            return Err(CrivError::usage(
                "only one search mode may be used at a time",
            ));
        }
        self.mode = Some(mode);
        Ok(())
    }
}

#[derive(Debug)]
struct Row {
    path: String,
    line: Option<usize>,
    text: String,
}

pub(crate) fn run(root: &Path, options: SearchOptions) -> Result<()> {
    let vault = Vault::load(root)?;
    let rows = match options.mode.as_ref().expect("checked") {
        Mode::Grep(text) => grep(root, &vault, text, &options.paths)?,
        Mode::Files(query) => files(&vault, query),
        Mode::Notes(text) => notes(&vault, text, options.semantic),
        Mode::Structural(pattern) => {
            return Err(CrivError::new(format!(
                "structural ast-grep search for `{pattern}` is not wired yet; use `--grep` for lexical source search in this MVP"
            )));
        }
        Mode::Rule(rule) => {
            return Err(CrivError::new(format!(
                "ADR policy rule search for `{rule}` is not wired yet"
            )));
        }
        Mode::PatternId(pattern_id) => {
            return Err(CrivError::new(format!(
                "registered pattern search for `{pattern_id}` is not wired yet"
            )));
        }
    };

    print_rows(&rows, options.format);
    Ok(())
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
    let needle = text.to_lowercase();
    let mut rows = Vec::new();
    for note in &vault.notes {
        let title_match = note
            .title
            .as_deref()
            .is_some_and(|title| title.to_lowercase().contains(&needle));
        if title_match || note.body.to_lowercase().contains(&needle) {
            rows.push(Row {
                path: note.rel_path.clone(),
                line: None,
                text: note.title.clone().unwrap_or_default(),
            });
        }
    }
    if semantic {
        rows.push(Row {
            path: "semantic".into(),
            line: None,
            text: "semantic embeddings are not enabled in this MVP".into(),
        });
    }
    rows
}

fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| glob_matches(pattern, path))
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
}

use std::ops::Range;
use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{CrivError, Result};

#[cfg(test)]
pub(crate) fn copy_fixture_tree(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_fixture_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn normalize_rel(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn strip_prefix(path: &Path, root: &Path) -> String {
    normalize_rel(path.strip_prefix(root).unwrap_or(path))
}

pub(crate) fn kebab(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub(crate) fn is_adr_id(value: &str) -> bool {
    value.len() == 8
        && value.starts_with("ADR-")
        && value[4..].chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn find_wiki_links_with_lines(markdown: &str) -> Vec<(usize, String, Range<usize>)> {
    let mut in_code_block = false;
    let mut code_ranges = Vec::new();

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_ranges.push(range);
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                code_ranges.push(range);
            }
            Event::Code(_) => code_ranges.push(range),
            _ if in_code_block => code_ranges.push(range),
            _ => {}
        }
    }

    let mut links = Vec::new();
    let mut start = 0;
    while let Some(open) = markdown[start..].find("[[") {
        let open = start + open;
        let body_start = open + 2;
        if in_ranges(open, &code_ranges) {
            start = body_start;
            continue;
        }
        if let Some(close) = markdown[body_start..].find("]]") {
            let close = body_start + close;
            if !in_ranges(close, &code_ranges) {
                links.push((
                    line_number(markdown, open),
                    markdown[body_start..close].to_string(),
                    open..close + 2,
                ));
            }
            start = close + 2;
        } else {
            break;
        }
    }
    links
}

pub(crate) fn markdown_headings(markdown: &str) -> Vec<(usize, String, usize)> {
    let mut headings = Vec::new();
    let mut active: Option<(usize, usize, String)> = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                active = Some((
                    heading_level(level),
                    line_number(markdown, range.start),
                    String::new(),
                ));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, heading)) = &mut active {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, line, text)) = active.take() {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        headings.push((level, text, line));
                    }
                }
            }
            _ => {}
        }
    }
    headings
}

#[cfg(test)]
fn glob_matches(pattern: &str, value: &str) -> bool {
    let patterns = [pattern.to_string()];
    GlobMatcher::new(&patterns).is_ok_and(|matcher| matcher.is_match(value))
}

#[derive(Debug, Clone)]
pub(crate) struct GlobMatcher {
    sets: Vec<(GlobSet, Vec<usize>)>,
}

impl GlobMatcher {
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        Self::from_patterns(patterns, (0..patterns.len()).collect())
    }

    /// Compiles every valid pattern and preserves its original index. This is
    /// for legacy matching paths where an invalid glob has always meant
    /// "does not match", rather than a validation error.
    pub(crate) fn from_valid_patterns(patterns: &[String]) -> Self {
        let mut valid = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            if GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .is_ok()
            {
                valid.push((index, pattern.clone()));
            }
        }
        match Self::from_patterns(
            &valid
                .iter()
                .map(|(_, pattern)| pattern.clone())
                .collect::<Vec<_>>(),
            valid.iter().map(|(index, _)| *index).collect(),
        ) {
            Ok(matcher) => matcher,
            // A valid aggregate can exceed globset's automaton limit. Keep the
            // tolerant contract by compiling each valid pattern independently.
            Err(_) => Self {
                sets: valid
                    .iter()
                    .filter_map(|(index, pattern)| {
                        Self::from_patterns(std::slice::from_ref(pattern), vec![*index]).ok()
                    })
                    .flat_map(|matcher| matcher.sets)
                    .collect(),
            },
        }
    }

    fn from_patterns(patterns: &[String], pattern_indices: Vec<usize>) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .backslash_escape(true)
                    .build()
                    .map_err(|err| CrivError::new(format!("invalid glob `{pattern}`: {err}")))?,
            );
        }
        Ok(Self {
            sets: vec![(
                builder
                    .build()
                    .map_err(|err| CrivError::new(format!("failed to compile globs: {err}")))?,
                pattern_indices,
            )],
        })
    }

    pub(crate) fn is_match(&self, value: &str) -> bool {
        self.sets.iter().any(|(set, _)| set.is_match(value))
    }

    pub(crate) fn matching_pattern_indices_into(&self, value: &str, into: &mut Vec<usize>) {
        into.clear();
        let mut matched = Vec::new();
        for (set, pattern_indices) in &self.sets {
            // globset clears `matched` before every call, so it is safe to
            // reuse this scratch allocation while accumulating all sets.
            set.matches_into(value, &mut matched);
            into.extend(matched.iter().map(|index| pattern_indices[*index]));
        }
    }
}

fn in_ranges(byte_offset: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| byte_offset >= range.start && byte_offset < range.end)
}

fn line_number(markdown: &str, byte_offset: usize) -> usize {
    markdown[..byte_offset.min(markdown.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn heading_level(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_links_include_line_numbers() {
        let links = find_wiki_links_with_lines("a [[one]]\nb [[two|Two]]");
        assert_eq!(
            links,
            vec![(1, "one".into(), 2..9), (2, "two|Two".into(), 12..23)]
        );
    }

    #[test]
    fn wiki_links_ignore_code_examples() {
        let links = find_wiki_links_with_lines("`[[example]]`\n[[real]]\n```\n[[fenced]]\n```");
        assert_eq!(links, vec![(2, "real".into(), 14..22)]);
    }

    #[test]
    fn simple_globs_match_repo_paths() {
        assert!(glob_matches("src/**", "src/auth/verify.rs"));
        assert!(glob_matches("src/*.rs", "src/lib.rs"));
        assert!(!glob_matches("src/*.rs", "src/auth/lib.rs"));
    }
}

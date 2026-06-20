use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{CrivError, Result};

pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?)
}

pub(crate) fn is_text_file(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::with_capacity(8192);
    Read::by_ref(&mut file)
        .take(8192)
        .read_to_end(&mut buffer)?;
    Ok(content_inspector::inspect(&buffer).is_text())
}

pub(crate) fn write_new(path: &Path, contents: &str) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(true)
}

pub(crate) fn append_line_if_missing(path: &Path, line: &str) -> Result<()> {
    let mut contents = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    if !contents.lines().any(|existing| existing.trim() == line) {
        if !contents.is_empty() && !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(line);
        contents.push('\n');
        fs::write(path, contents)?;
    }

    Ok(())
}

pub(crate) fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| CrivError::new(format!("cannot write atomic file at {}", path.display())))?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    for attempt in 0..100 {
        let temp_path = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            nonce + attempt
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        };

        let write_result = file
            .write_all(contents.as_bytes())
            .and_then(|_| file.sync_all());
        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }
        if let Err(err) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }
        return Ok(());
    }

    Err(CrivError::new(format!(
        "failed to create temporary file for {}",
        path.display()
    )))
}

pub(crate) fn walk_files(root: &Path, extension: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    walk_files_inner(root, extension, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_files_inner(root: &Path, extension: Option<&str>, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == ".criv" || name == "target" || name == "node_modules" {
            continue;
        }

        if path.is_dir() {
            walk_files_inner(&path, extension, files)?;
        } else if extension.is_none_or(|ext| path.extension().is_some_and(|value| value == ext)) {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn normalize_rel(path: &Path) -> String {
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

pub(crate) fn find_wiki_links_with_lines(markdown: &str) -> Vec<(usize, String)> {
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
                ));
            }
            start = close + 2;
        } else {
            break;
        }
    }
    links
}

pub(crate) fn markdown_fenced_blocks(markdown: &str) -> Vec<(usize, Option<String>, String)> {
    let mut blocks = Vec::new();
    let mut active: Option<(usize, Option<String>, String)> = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                active = Some((
                    line_number(markdown, range.start),
                    Some(info.trim().to_string()).filter(|value| !value.is_empty()),
                    String::new(),
                ));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => {
                active = Some((line_number(markdown, range.start), None, String::new()));
            }
            Event::Text(text) => {
                if let Some((_, _, contents)) = &mut active {
                    contents.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(block) = active.take() {
                    blocks.push(block);
                }
            }
            _ => {}
        }
    }

    blocks
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

pub(crate) fn glob_matches(pattern: &str, value: &str) -> bool {
    let patterns = [pattern.to_string()];
    GlobMatcher::new(&patterns).is_ok_and(|matcher| matcher.is_match(value))
}

#[derive(Debug, Clone)]
pub(crate) struct GlobMatcher {
    set: GlobSet,
}

impl GlobMatcher {
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .map_err(|err| CrivError::new(format!("invalid glob `{pattern}`: {err}")))?;
            builder.add(glob);
        }
        Ok(Self {
            set: builder
                .build()
                .map_err(|err| CrivError::new(format!("failed to compile globs: {err}")))?,
        })
    }

    pub(crate) fn is_match(&self, value: &str) -> bool {
        self.set.is_match(value)
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
        assert_eq!(links, vec![(1, "one".into()), (2, "two|Two".into())]);
    }

    #[test]
    fn wiki_links_ignore_code_examples() {
        let links = find_wiki_links_with_lines("`[[example]]`\n[[real]]\n```\n[[fenced]]\n```");
        assert_eq!(links, vec![(2, "real".into())]);
    }

    #[test]
    fn simple_globs_match_repo_paths() {
        assert!(glob_matches("src/**", "src/auth/verify.rs"));
        assert!(glob_matches("src/*.rs", "src/lib.rs"));
        assert!(!glob_matches("src/*.rs", "src/auth/lib.rs"));
    }

    #[test]
    fn fenced_blocks_include_line_info_and_contents() {
        let blocks = markdown_fenced_blocks("intro\n```mermaid\nflowchart TD\n```\n\n    code\n");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, 2);
        assert_eq!(blocks[0].1.as_deref(), Some("mermaid"));
        assert_eq!(blocks[0].2, "flowchart TD\n");
        assert_eq!(blocks[1].1, None);
        assert_eq!(blocks[1].2, "code\n");
    }

    #[test]
    fn atomic_write_replaces_existing_file_contents() {
        let root = std::env::temp_dir().join(format!(
            "criv-atomic-write-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        write_atomic(&path, "{\"old\":true}\n").unwrap();
        write_atomic(&path, "{\"new\":true}\n").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"new\":true}\n");
        let leftovers = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(leftovers, vec!["state.json".to_string()]);

        let _ = std::fs::remove_dir_all(root);
    }
}

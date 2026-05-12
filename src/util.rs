use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;

pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?)
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
    let mut links = Vec::new();
    let mut fenced = false;
    for (line_index, line) in markdown.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }

        let mut start = 0;
        while let Some(open) = line[start..].find("[[") {
            let open = start + open;
            if inside_inline_code(line, open) {
                start = open + 2;
                continue;
            }
            let body_start = open + 2;
            if let Some(close) = line[body_start..].find("]]") {
                let close = body_start + close;
                links.push((line_index + 1, line[body_start..close].to_string()));
                start = close + 2;
            } else {
                break;
            }
        }
    }
    links
}

fn inside_inline_code(line: &str, byte_index: usize) -> bool {
    let mut ticks = 0;
    for (index, ch) in line.char_indices() {
        if index >= byte_index {
            break;
        }
        if ch == '`' {
            ticks += 1;
        }
    }
    ticks % 2 == 1
}

pub(crate) fn glob_matches(pattern: &str, value: &str) -> bool {
    glob_parts(
        &pattern.split('/').collect::<Vec<_>>(),
        &value.split('/').collect::<Vec<_>>(),
    )
}

fn glob_parts(pattern: &[&str], value: &[&str]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    if pattern[0] == "**" {
        return glob_parts(&pattern[1..], value)
            || (!value.is_empty() && glob_parts(pattern, &value[1..]));
    }
    !value.is_empty()
        && glob_segment(pattern[0], value[0])
        && glob_parts(&pattern[1..], &value[1..])
}

fn glob_segment(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut p = 0;
    let mut v = 0;
    let mut star = None;
    let mut star_match = 0;

    while v < value.len() {
        if p < pattern.len() && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_match = v;
        } else if let Some(star_pos) = star {
            p = star_pos + 1;
            star_match += 1;
            v = star_match;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
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
}

use std::path::Path;

fn normalize_rel(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn strip_prefix(path: &Path, root: &Path) -> String {
    normalize_rel(path.strip_prefix(root).unwrap_or(path))
}

/// The heading slug: lowercase alphanumeric words joined by `-`.
pub fn slug(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

pub const MAX_ADR_NUMBER: u32 = 9999;

/// The number of an `ADR-NNNN` id.
pub fn parse_adr_id(value: &str) -> Option<u32> {
    parse_adr_number(value.strip_prefix("ADR-")?)
}

pub fn is_adr_id(value: &str) -> bool {
    parse_adr_id(value).is_some()
}

pub fn format_adr_id(number: u32) -> String {
    format!("ADR-{number:04}")
}

pub fn adr_filename(number: u32, slug: &str) -> String {
    format!("{number:04}-{slug}.md")
}

/// Split an ADR file stem, `NNNN` or `NNNN-slug`, into its number and slug.
pub fn parse_adr_stem(stem: &str) -> Option<(u32, Option<&str>)> {
    let (number, slug) = stem
        .split_once('-')
        .map_or((stem, None), |(number, slug)| (number, Some(slug)));
    Some((parse_adr_number(number)?, slug))
}

/// Split the stem of an ADR path or link target, with or without `.md`.
pub fn parse_adr_path(path: &str) -> Option<(u32, Option<&str>)> {
    let name = path.rsplit('/').next().unwrap_or(path);
    parse_adr_stem(name.strip_suffix(".md").unwrap_or(name))
}

fn parse_adr_number(digits: &str) -> Option<u32> {
    (digits.len() == 4 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// The full id of an ADR policy pattern: `ADR-NNNN/local-id`. Surrounding
/// whitespace in the local id is not part of the id.
pub fn pattern_id(adr_id: &str, local_id: &str) -> String {
    format!("{adr_id}/{}", local_id.trim())
}

/// Split a full pattern id into its ADR id and its non-empty local id.
pub fn split_pattern_id(pattern_id: &str) -> Option<(&str, &str)> {
    let (adr_id, local_id) = pattern_id.split_once('/')?;
    let local_id = local_id.trim();
    (is_adr_id(adr_id) && !local_id.is_empty()).then_some((adr_id, local_id))
}

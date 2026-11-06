//! Exact rewriting of the `governs:` block sequence in ADR frontmatter.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::markdown;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy)]
enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
}

#[derive(Debug)]
struct ScalarSpan {
    value: String,
    range: Range<usize>,
    style: ScalarStyle,
}

/// Whether `new` is the accepted decision `old` with only its `governs:`
/// values replaced.
pub(super) fn transition_matches(
    old: &str,
    new: &str,
    replacements: &BTreeMap<String, String>,
) -> bool {
    let accepted = |contents: &str| {
        markdown::split_frontmatter(contents)
            .and_then(|frontmatter| frontmatter.scalar("status"))
            .is_some_and(|status| status == "accepted")
    };
    if !accepted(old) || !accepted(new) {
        return false;
    }
    rewrite(old, replacements).is_ok_and(|rewritten| rewritten == new && rewritten != old)
}

/// Replace each `governs:` value named in `replacements`, keeping its quoting.
pub(super) fn rewrite(contents: &str, replacements: &BTreeMap<String, String>) -> Result<String> {
    let spans = scalars(contents)?;
    let mut output = contents.to_string();
    for span in spans.iter().rev() {
        if let Some(replacement) = replacements.get(&span.value) {
            output.replace_range(span.range.clone(), &render_scalar(replacement, span.style));
        }
    }
    Ok(output)
}

/// The `governs:` values of `old` that `replacements` rewrote into `new`.
pub(super) fn replaced_values(
    old: &str,
    new: &str,
    replacements: &BTreeMap<String, String>,
) -> Result<BTreeSet<String>> {
    if rewrite(old, replacements)? != new {
        return Err(CrivError::new(
            "source reconciliation produced a non-exact `governs:` rewrite",
        ));
    }
    Ok(scalars(old)?
        .into_iter()
        .map(|span| span.value)
        .filter(|value| replacements.contains_key(value))
        .collect())
}

fn scalars(contents: &str) -> Result<Vec<ScalarSpan>> {
    let Some(frontmatter) = markdown::split_frontmatter(contents) else {
        return Err(CrivError::new("ADR has no closed YAML frontmatter"));
    };
    let lines = indexed_lines(frontmatter.yaml, frontmatter.yaml_start);
    let mut field = None;
    for (index, (_, line)) in lines.iter().enumerate() {
        let line = *line;
        if line.starts_with(char::is_whitespace)
            || line.trim().is_empty()
            || line.trim_start().starts_with('#')
        {
            continue;
        }
        if line == "governs:" {
            if field.replace(index).is_some() {
                return Err(CrivError::new("ADR has duplicate `governs:` fields"));
            }
        } else if line.starts_with("governs:") {
            return Err(CrivError::new("`governs:` must use a block YAML sequence"));
        }
    }
    let Some(field) = field else {
        return Ok(Vec::new());
    };
    let mut spans = Vec::new();
    for (line_start, line) in lines.iter().skip(field.saturating_add(1)) {
        if !line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(raw) = trimmed.strip_prefix("- ") else {
            return Err(CrivError::new("`governs:` contains unsupported YAML"));
        };
        let raw = raw.trim_end();
        if raw.is_empty() {
            return Err(CrivError::new("`governs:` contains an empty scalar"));
        }
        let value = parse_scalar(raw)?;
        let style = if raw.starts_with('\'') {
            ScalarStyle::SingleQuoted
        } else if raw.starts_with('"') {
            ScalarStyle::DoubleQuoted
        } else {
            ScalarStyle::Plain
        };
        let raw_offset = line
            .find(raw)
            .ok_or_else(|| CrivError::new("cannot locate `governs:` scalar"))?;
        let range_start = line_start
            .checked_add(raw_offset)
            .ok_or_else(|| CrivError::new("`governs:` scalar position exceeds address space"))?;
        let range_end = range_start
            .checked_add(raw.len())
            .ok_or_else(|| CrivError::new("`governs:` scalar range exceeds address space"))?;
        spans.push(ScalarSpan {
            value,
            range: range_start..range_end,
            style,
        });
    }
    Ok(spans)
}

/// Each line of `text` without its terminator, with its offset in the file
/// that starts `start` bytes before `text`.
fn indexed_lines(text: &str, start: usize) -> Vec<(usize, &str)> {
    let mut offset = start;
    text.split_inclusive('\n')
        .map(|line| {
            let line_start = offset;
            offset = offset.saturating_add(line.len());
            (line_start, line.trim_end_matches(['\r', '\n']))
        })
        .collect()
}

fn parse_scalar(raw: &str) -> Result<String> {
    if !raw.starts_with(['\'', '"'])
        && (raw.contains(" #") || raw.starts_with(['[', '{', '&', '*', '!', '|', '>', '@', '`']))
    {
        return Err(CrivError::new(
            "`governs:` contains an unsupported YAML scalar",
        ));
    }
    serde_norway::from_str::<String>(raw)
        .map_err(|error| CrivError::new(format!("cannot parse `governs:` scalar: {error}")))
}

fn render_scalar(value: &str, style: ScalarStyle) -> String {
    match style {
        ScalarStyle::Plain => value.to_string(),
        ScalarStyle::SingleQuoted => format!("'{}'", value.replace('\'', "''")),
        ScalarStyle::DoubleQuoted => {
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replacements(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(old, new)| ((*old).to_owned(), (*new).to_owned()))
            .collect()
    }

    #[test]
    fn rewrites_plain_and_quoted_governs_scalars_only() {
        let input = "---\nid: ADR-0001\nstatus: accepted\ngoverns:\n  - src/old.rs\n  - 'src/second.rs'\n  - \"src/third.rs\"\n---\n\nsrc/old.rs stays in prose\n";
        let output = rewrite(
            input,
            &replacements(&[
                ("src/old.rs", "src/new.rs"),
                ("src/second.rs", "src/second-new.rs"),
                ("src/third.rs", "src/third-new.rs"),
            ]),
        )
        .unwrap();
        assert!(output.contains("  - src/new.rs\n"));
        assert!(output.contains("  - 'src/second-new.rs'\n"));
        assert!(output.contains("  - \"src/third-new.rs\"\n"));
        assert!(output.ends_with("src/old.rs stays in prose\n"));
    }

    #[test]
    fn refuses_inline_or_nested_governs_yaml() {
        let inline = "---\nstatus: accepted\ngoverns: [src/old.rs]\n---\n";
        assert!(rewrite(inline, &BTreeMap::new()).is_err());
        let nested = "---\nstatus: accepted\ngoverns:\n  path: src/old.rs\n---\n";
        assert!(rewrite(nested, &BTreeMap::new()).is_err());
    }

    #[test]
    fn transition_rejects_any_non_governs_change() {
        let old =
            "---\nid: ADR-0001\nstatus: accepted\ngoverns:\n  - src/old.rs\n---\n\nDecision\n";
        let new = old.replace("src/old.rs", "src/new.rs");
        let mapping = replacements(&[("src/old.rs", "src/new.rs")]);
        assert!(transition_matches(old, &new, &mapping));
        assert!(!transition_matches(old, &(new + "changed\n"), &mapping));
    }

    #[test]
    fn preserves_case_sensitive_path_replacements() {
        let old = "---\nstatus: accepted\ngoverns:\n  - src/name.rs\n---\n";
        let new = "---\nstatus: accepted\ngoverns:\n  - src/Name.rs\n---\n";
        assert!(transition_matches(
            old,
            new,
            &replacements(&[("src/name.rs", "src/Name.rs")])
        ));
    }
}

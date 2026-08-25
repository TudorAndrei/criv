//! Diagnostic identity, repairs, and the exact source locations shared by
//! diagnostic producers and output adapters.

use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub fn fix_for(code: &str) -> Option<&'static str> {
    let fix = match code {
        "adr-dir-non-decision" => {
            "Set `kind: decision` in the frontmatter, or move the file out of the ADR directory."
        }
        "adr-filename" => "Rename the file to `NNNN-kebab-title.md`.",
        "adr-immutability-violation" => {
            "Restore the accepted ADR, and record the change in a new ADR with `supersedes:`."
        }
        "ambiguous-policy-pattern-body" => {
            "Give the pattern either `pattern` or `rule`, and not both."
        }
        "ambiguous-source-link" => {
            "Name the file in the link, as in `[[src/lib.rs#fn:run]]`, so one source resolves."
        }
        "architecture-interface-drift" => {
            "Run `criv watch --once`, then correct the C4 element so it matches the source."
        }
        "broken-link" => "Correct the link target, or add the note it names.",
        "check-failed" => "Repair the diagnostics above, then run `criv check`.",
        "decision-location" => "Move the decision into the ADR directory.",
        "duplicate-doc-pattern" | "duplicate-pattern-id" => "Give each pattern a unique `id`.",
        "duplicate-id" => "Give the note an `id` no other note uses.",
        "duplicate-policy-pattern" => "Give each policy pattern in the ADR a unique `id`.",
        "empty-policy-pattern" => "Give the policy pattern a body, or remove it.",
        "empty-target-scope" => "Name at least one path or symbol under `targets`.",
        "enforcement-failed" => "Run `criv check` for the diagnostics, then repair them.",
        "import-policy-violation" => "Remove the import, or widen the import policy in criv.toml.",
        "inconsistent-supersession" => {
            "Make `supersedes` and `superseded_by` agree in both decisions."
        }
        "invalid-adr-id" => "Take a free id from `criv query next-adr-id`.",
        "invalid-frontmatter" => "Correct the YAML frontmatter block.",
        "invalid-kind" => "Set `kind` to `doc` or to `decision`.",
        "invalid-likec4-source" => "Correct the LikeC4 source, then run `criv watch --once`.",
        "invalid-policy-pattern" => "Correct the ast-grep pattern or rule syntax.",
        "legacy-source-target" | "source-wikilink" => {
            "Rewrite the target as an AST-aware selector, as in `src/main.rs#fn:run`."
        }
        "markdown-format" => "Run `criv check --fix`.",
        "missing-id" => "Add an `id` to the note frontmatter.",
        "missing-policy-pattern-body" => "Give the pattern a `pattern` or a `rule`.",
        "missing-policy-pattern-definition" => {
            "Declare the pattern in an ADR `policy.patterns` entry."
        }
        "missing-policy-pattern-id" => "Give the policy pattern an `id`.",
        "missing-policy-pattern-language" => "Set `language` on the policy pattern.",
        "non-portable-note-link" => "Use the portable link form `[[note-id|Text]]`.",
        "not-a-vault" => "criv init",
        "policy-violation" => "Change the code, or write a successor ADR that retires the policy.",
        "supersession-cycle" => "Break the cycle: a decision cannot supersede its own ancestor.",
        "unknown-superseded-by" | "unknown-supersedes" => {
            "Name a decision that exists, or add the missing ADR."
        }
        "unresolved-governs" => {
            "Run `criv adr reconcile-sources --base <ref>` for a rename, or add a successor ADR for a deletion."
        }
        "unresolved-pattern" => "Correct the pattern reference, or declare the pattern.",
        "unresolved-target" => {
            "Correct the target, or run `criv watch --once` to refresh the state."
        }
        _ => return None,
    };
    Some(fix)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ByteSpan {
    start: usize,
    end: usize,
}

impl ByteSpan {
    fn new(source: &str, range: Range<usize>) -> Option<Self> {
        (range.start <= range.end
            && range.end <= source.len()
            && source.is_char_boundary(range.start)
            && source.is_char_boundary(range.end))
        .then_some(Self {
            start: range.start,
            end: range.end,
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
pub struct LspPosition {
    pub(crate) line: usize,
    pub(crate) character: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
pub struct LspRange {
    pub(crate) start: LspPosition,
    pub(crate) end: LspPosition,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct GithubLocation {
    pub(crate) line: usize,
    pub(crate) column: Option<usize>,
    pub(crate) end_line: usize,
    pub(crate) end_column: Option<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceLocation {
    source: Arc<str>,
    span: ByteSpan,
}

impl SourceLocation {
    pub(crate) fn new(source: Arc<str>, range: Range<usize>) -> Option<Self> {
        let span = ByteSpan::new(&source, range)?;
        Some(Self { source, span })
    }

    pub(crate) fn from_lsp_range(source: Arc<str>, range: LspRange) -> Option<Self> {
        if (range.end.line, range.end.character) < (range.start.line, range.start.character) {
            return None;
        }
        let start = lsp_position_to_byte(&source, range.start)?;
        let end = lsp_position_to_byte(&source, range.end)?;
        Self::new(source, start..end)
    }

    pub(crate) fn from_one_based_character_range(
        source: Arc<str>,
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Option<Self> {
        let start = one_based_character_position_to_byte(&source, line, column)?;
        let end = one_based_character_position_to_byte(&source, end_line, end_column)?;
        Self::new(source, start..end)
    }

    pub(crate) fn line(&self) -> Option<usize> {
        self.lsp_range()?.start.line.checked_add(1)
    }

    pub(crate) fn lsp_range(&self) -> Option<LspRange> {
        Some(LspRange {
            start: byte_to_lsp_position(&self.source, self.span.start)?,
            end: byte_to_lsp_position(&self.source, self.span.end)?,
        })
    }

    pub(crate) fn github_location(&self) -> Option<GithubLocation> {
        let start = byte_to_character_position(&self.source, self.span.start)?;
        let end = if self.span.start == self.span.end {
            start
        } else {
            inclusive_end_position(&self.source, self.span.end)?
        };
        let same_line = start.line == end.line;
        Some(GithubLocation {
            line: start.line.checked_add(1)?,
            column: same_line.then(|| start.character.checked_add(1)).flatten(),
            end_line: end.line.checked_add(1)?,
            end_column: same_line.then(|| end.character.checked_add(1)).flatten(),
        })
    }
}

fn inclusive_end_position(source: &str, end: usize) -> Option<LspPosition> {
    let (offset, character) = source.get(..end)?.char_indices().next_back()?;
    if character == '\n'
        || character == '\r' && source.as_bytes().get(offset.checked_add(1)?) == Some(&b'\n')
    {
        let line = source
            .get(..offset)?
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        let (line_start, line_end) = line_bounds(source, line)?;
        return Some(LspPosition {
            line,
            character: source
                .get(line_start..line_end)?
                .chars()
                .count()
                .saturating_sub(1),
        });
    }
    byte_to_character_position(source, offset)
}

fn byte_to_lsp_position(source: &str, offset: usize) -> Option<LspPosition> {
    let position = byte_to_character_position(source, offset)?;
    let (line_start, _) = line_bounds(source, position.line)?;
    let mut prefix = source.get(line_start..offset)?;
    if prefix.ends_with('\r') && source.as_bytes().get(offset) == Some(&b'\n') {
        prefix = prefix.strip_suffix('\r')?;
    }
    Some(LspPosition {
        line: position.line,
        character: prefix.chars().map(char::len_utf16).sum(),
    })
}

fn byte_to_character_position(source: &str, offset: usize) -> Option<LspPosition> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }
    let prefix = source.get(..offset)?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix
        .rfind('\n')
        .map_or(Some(0), |newline| newline.checked_add(1))?;
    let mut line_prefix = source.get(line_start..offset)?;
    if line_prefix.ends_with('\r') && source.as_bytes().get(offset) == Some(&b'\n') {
        line_prefix = line_prefix.strip_suffix('\r')?;
    }
    Some(LspPosition {
        line,
        character: line_prefix.chars().count(),
    })
}

fn lsp_position_to_byte(source: &str, position: LspPosition) -> Option<usize> {
    let (line_start, line_end) = line_bounds(source, position.line)?;
    let line = source.get(line_start..line_end)?;
    let mut utf16 = 0;
    for (offset, character) in line.char_indices() {
        if utf16 == position.character {
            return line_start.checked_add(offset);
        }
        utf16 = utf16.checked_add(character.len_utf16())?;
        if utf16 > position.character {
            return None;
        }
    }
    (utf16 == position.character).then_some(line_end)
}

fn one_based_character_position_to_byte(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let (line_start, line_end) = line_bounds(source, line.checked_sub(1)?)?;
    let line = source.get(line_start..line_end)?;
    let character = column.checked_sub(1)?;
    line.char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(line.len()))
        .nth(character)
        .and_then(|offset| line_start.checked_add(offset))
}

fn line_bounds(source: &str, target_line: usize) -> Option<(usize, usize)> {
    let mut line = 0;
    let mut start = 0;
    while line < target_line {
        let newline = source.get(start..)?.find('\n')?;
        start = start.checked_add(newline)?.checked_add(1)?;
        line = line.checked_add(1)?;
    }
    let mut end = source
        .get(start..)?
        .find('\n')
        .map_or(Some(source.len()), |newline| start.checked_add(newline))?;
    if end > start && source.as_bytes().get(end.checked_sub(1)?) == Some(&b'\r') {
        end = end.checked_sub(1)?;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_utf8_boundaries_and_order() {
        let source = "a😀b";
        assert!(ByteSpan::new(source, 1..5).is_some());
        assert!(ByteSpan::new(source, 2..5).is_none());
        assert!(ByteSpan::new(source, Range { start: 5, end: 4 }).is_none());
        assert!(ByteSpan::new(source, 0..20).is_none());
    }

    #[test]
    fn converts_unicode_crlf_multiline_and_empty_ranges() {
        let source: Arc<str> = Arc::from("a😀b\r\nçd\n");
        let same_line = SourceLocation::new(source.clone(), 1..6).unwrap();
        assert_eq!(
            same_line.lsp_range(),
            Some(LspRange {
                start: LspPosition {
                    line: 0,
                    character: 1,
                },
                end: LspPosition {
                    line: 0,
                    character: 4,
                },
            })
        );
        assert_eq!(
            same_line.github_location(),
            Some(GithubLocation {
                line: 1,
                column: Some(2),
                end_line: 1,
                end_column: Some(3),
            })
        );

        let multiline = SourceLocation::new(source.clone(), 5..11).unwrap();
        assert_eq!(
            multiline.lsp_range(),
            Some(LspRange {
                start: LspPosition {
                    line: 0,
                    character: 3,
                },
                end: LspPosition {
                    line: 1,
                    character: 2,
                },
            })
        );
        assert_eq!(
            multiline.github_location(),
            Some(GithubLocation {
                line: 1,
                column: None,
                end_line: 2,
                end_column: None,
            })
        );

        let empty = SourceLocation::new(source, 8..8).unwrap();
        let range = empty.lsp_range().unwrap();
        assert_eq!(range.start, range.end);
        assert_eq!(
            empty.github_location(),
            Some(GithubLocation {
                line: 2,
                column: Some(1),
                end_line: 2,
                end_column: Some(1),
            })
        );
    }

    #[test]
    fn converts_and_rejects_external_coordinate_ranges() {
        let source: Arc<str> = Arc::from("a😀b\r\nçd\n");
        let lsp = LspRange {
            start: LspPosition {
                line: 0,
                character: 1,
            },
            end: LspPosition {
                line: 1,
                character: 1,
            },
        };
        let location = SourceLocation::from_lsp_range(source.clone(), lsp).unwrap();
        assert_eq!(location.lsp_range(), Some(lsp));
        assert!(
            SourceLocation::from_lsp_range(
                source.clone(),
                LspRange {
                    start: LspPosition {
                        line: 0,
                        character: 2,
                    },
                    end: LspPosition {
                        line: 0,
                        character: 3,
                    },
                },
            )
            .is_none(),
            "a UTF-16 position inside a surrogate pair is invalid"
        );

        let character_range =
            SourceLocation::from_one_based_character_range(source, 1, 2, 1, 4).unwrap();
        assert_eq!(
            character_range.lsp_range(),
            Some(LspRange {
                start: LspPosition {
                    line: 0,
                    character: 1,
                },
                end: LspPosition {
                    line: 0,
                    character: 4,
                },
            })
        );
    }
}

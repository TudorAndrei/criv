//! Exact source locations shared by diagnostic producers and output adapters.

use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ByteSpan {
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
pub(crate) struct LspPosition {
    pub(crate) line: usize,
    pub(crate) character: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct LspRange {
    pub(crate) start: LspPosition,
    pub(crate) end: LspPosition,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct GithubLocation {
    pub(crate) line: usize,
    pub(crate) column: Option<usize>,
    pub(crate) end_line: usize,
    pub(crate) end_column: Option<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SourceLocation {
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

    pub(crate) fn line(&self) -> usize {
        self.lsp_range().start.line + 1
    }

    pub(crate) fn lsp_range(&self) -> LspRange {
        LspRange {
            start: byte_to_lsp_position(&self.source, self.span.start)
                .expect("validated byte-span start"),
            end: byte_to_lsp_position(&self.source, self.span.end)
                .expect("validated byte-span end"),
        }
    }

    pub(crate) fn github_location(&self) -> GithubLocation {
        let start = byte_to_character_position(&self.source, self.span.start)
            .expect("validated byte-span start");
        let end = if self.span.start == self.span.end {
            start
        } else {
            inclusive_end_position(&self.source, self.span.end)
        };
        let same_line = start.line == end.line;
        GithubLocation {
            line: start.line + 1,
            column: same_line.then_some(start.character + 1),
            end_line: end.line + 1,
            end_column: same_line.then_some(end.character + 1),
        }
    }
}

fn inclusive_end_position(source: &str, end: usize) -> LspPosition {
    let (offset, character) = source[..end]
        .char_indices()
        .next_back()
        .expect("a non-empty span has a preceding character");
    if character == '\n' || character == '\r' && source.as_bytes().get(offset + 1) == Some(&b'\n') {
        let line = source[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        let (line_start, line_end) = line_bounds(source, line).expect("newline owns a line");
        return LspPosition {
            line,
            character: source[line_start..line_end]
                .chars()
                .count()
                .saturating_sub(1),
        };
    }
    byte_to_character_position(source, offset).expect("char index is a character boundary")
}

fn byte_to_lsp_position(source: &str, offset: usize) -> Option<LspPosition> {
    let position = byte_to_character_position(source, offset)?;
    let (line_start, _) = line_bounds(source, position.line)?;
    let mut prefix = &source[line_start..offset];
    if prefix.ends_with('\r') && source.as_bytes().get(offset) == Some(&b'\n') {
        prefix = &prefix[..prefix.len() - 1];
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
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    let mut line_prefix = &source[line_start..offset];
    if line_prefix.ends_with('\r') && source.as_bytes().get(offset) == Some(&b'\n') {
        line_prefix = &line_prefix[..line_prefix.len() - 1];
    }
    Some(LspPosition {
        line,
        character: line_prefix.chars().count(),
    })
}

fn lsp_position_to_byte(source: &str, position: LspPosition) -> Option<usize> {
    let (line_start, line_end) = line_bounds(source, position.line)?;
    let line = &source[line_start..line_end];
    let mut utf16 = 0;
    for (offset, character) in line.char_indices() {
        if utf16 == position.character {
            return Some(line_start + offset);
        }
        utf16 += character.len_utf16();
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
    let (line_start, line_end) = line_bounds(source, line - 1)?;
    let line = &source[line_start..line_end];
    let character = column - 1;
    line.char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(line.len()))
        .nth(character)
        .map(|offset| line_start + offset)
}

fn line_bounds(source: &str, target_line: usize) -> Option<(usize, usize)> {
    let mut line = 0;
    let mut start = 0;
    while line < target_line {
        let newline = source[start..].find('\n')?;
        start += newline + 1;
        line += 1;
    }
    let mut end = source[start..]
        .find('\n')
        .map_or(source.len(), |newline| start + newline);
    if end > start && source.as_bytes()[end - 1] == b'\r' {
        end -= 1;
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
            LspRange {
                start: LspPosition {
                    line: 0,
                    character: 1,
                },
                end: LspPosition {
                    line: 0,
                    character: 4,
                },
            }
        );
        assert_eq!(
            same_line.github_location(),
            GithubLocation {
                line: 1,
                column: Some(2),
                end_line: 1,
                end_column: Some(3),
            }
        );

        let multiline = SourceLocation::new(source.clone(), 5..11).unwrap();
        assert_eq!(
            multiline.lsp_range(),
            LspRange {
                start: LspPosition {
                    line: 0,
                    character: 3,
                },
                end: LspPosition {
                    line: 1,
                    character: 2,
                },
            }
        );
        assert_eq!(
            multiline.github_location(),
            GithubLocation {
                line: 1,
                column: None,
                end_line: 2,
                end_column: None,
            }
        );

        let empty = SourceLocation::new(source, 8..8).unwrap();
        assert_eq!(empty.lsp_range().start, empty.lsp_range().end);
        assert_eq!(
            empty.github_location(),
            GithubLocation {
                line: 2,
                column: Some(1),
                end_line: 2,
                end_column: Some(1),
            }
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
        assert_eq!(location.lsp_range(), lsp);
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
            LspRange {
                start: LspPosition {
                    line: 0,
                    character: 1,
                },
                end: LspPosition {
                    line: 0,
                    character: 4,
                },
            }
        );
    }
}

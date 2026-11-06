//! Markdown notation shared by the vault and the ADR tools: wiki-link spans,
//! the wiki-link body grammar, frontmatter, headings, and line numbers.

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

/// The byte ranges of one `[[body]]` occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLinkSpan {
    pub(crate) outer: Range<usize>,
    pub(crate) body: Range<usize>,
}

/// Every `[[body]]` in `text`, code included, in text order.
pub fn wiki_link_spans(text: &str) -> Vec<WikiLinkSpan> {
    scan_wiki_links(text, |_| false)
}

/// Scan `[[...]]` spans; an opening bracket that `skip_open` rejects is
/// skipped and the scan resumes just after it.
fn scan_wiki_links(text: &str, skip_open: impl Fn(usize) -> bool) -> Vec<WikiLinkSpan> {
    let mut spans = Vec::new();
    let mut start = 0;
    while let Some(tail) = text.get(start..) {
        let Some(open) = tail
            .find("[[")
            .and_then(|relative| start.checked_add(relative))
        else {
            break;
        };
        let Some(body_start) = open.checked_add(2) else {
            break;
        };
        if skip_open(open) {
            start = body_start;
            continue;
        }
        let Some(close) = text
            .get(body_start..)
            .and_then(|body| body.find("]]"))
            .and_then(|relative| body_start.checked_add(relative))
        else {
            break;
        };
        let Some(end) = close.checked_add(2) else {
            break;
        };
        spans.push(WikiLinkSpan {
            outer: open..end,
            body: body_start..close,
        });
        start = end;
    }
    spans
}

pub fn find_wiki_links_with_lines(markdown: &str) -> Vec<(usize, String, Range<usize>)> {
    let code_ranges = Parser::new(markdown)
        .into_offset_iter()
        .filter(|(event, _)| matches!(event, Event::Start(Tag::CodeBlock(_)) | Event::Code(_)))
        .map(|(_, range)| range)
        .collect::<Vec<_>>();

    scan_wiki_links(markdown, |open| in_ranges(open, &code_ranges))
        .into_iter()
        .filter(|span| !in_ranges(span.body.end, &code_ranges))
        .filter_map(|span| {
            let body = markdown.get(span.body)?;
            Some((
                line_number(markdown, span.outer.start),
                body.to_string(),
                span.outer,
            ))
        })
        .collect()
}

/// Split a wiki-link body at its first `|` into target and alias, untrimmed.
pub fn split_alias(body: &str) -> (&str, Option<&str>) {
    body.split_once('|')
        .map_or((body, None), |(target, alias)| (target, Some(alias)))
}

/// Split a link target at its first `#` into base and fragment, untrimmed.
pub fn split_fragment(target: &str) -> (&str, Option<&str>) {
    target
        .split_once('#')
        .map_or((target, None), |(base, fragment)| (base, Some(fragment)))
}

/// The grammar of a wiki-link body: `target[#fragment][|alias]`, where the
/// target may be `source:path[#symbol]`, `path#Lx-Ly`, `match:ADR-NNNN/id`,
/// or `note#match:ADR-NNNN/id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkTarget<'a> {
    target: &'a str,
}

impl<'a> LinkTarget<'a> {
    pub fn parse(body: &'a str) -> Self {
        Self {
            target: split_alias(body).0.trim(),
        }
    }

    /// The trimmed target without its alias.
    pub const fn as_str(self) -> &'a str {
        self.target
    }

    /// The target uses the explicit `source:` form.
    pub fn is_source(self) -> bool {
        self.target.starts_with("source:")
    }

    /// The target without a `source:` prefix.
    pub fn source_body(self) -> &'a str {
        self.target.strip_prefix("source:").unwrap_or(self.target)
    }

    /// The note or file part, before any `#` fragment.
    pub fn path(self) -> &'a str {
        split_fragment(self.source_body()).0.trim()
    }

    /// The non-empty `#` fragment: a heading, a symbol, or a line range.
    pub fn fragment(self) -> Option<&'a str> {
        split_fragment(self.source_body())
            .1
            .filter(|fragment| !fragment.is_empty())
    }

    /// The source symbol fragment; a `#Lx` or `#Lx-Ly` line range is not one.
    pub fn symbol(self) -> Option<&'a str> {
        self.fragment()
            .filter(|fragment| !is_line_fragment(fragment))
    }

    /// The path part is a `match:` pattern reference, not a file.
    pub fn path_is_pattern(self) -> bool {
        self.path().starts_with("match:")
    }

    /// The policy pattern id of a `match:` target.
    pub fn pattern_id(self) -> Option<&'a str> {
        self.target.strip_prefix("match:").or_else(|| {
            self.target
                .split_once("#match:")
                .map(|(_, pattern_id)| pattern_id)
        })
    }
}

fn is_line_fragment(fragment: &str) -> bool {
    let parse_line = |value: &str| {
        value
            .strip_prefix('L')
            .and_then(|line| line.parse::<usize>().ok())
            .is_some_and(|line| line > 0)
    };
    match fragment.split_once('-') {
        Some((start, end)) => parse_line(start) && parse_line(end),
        None => parse_line(fragment),
    }
}

/// A YAML frontmatter block between two `---` lines at the start of a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frontmatter<'a> {
    pub(crate) yaml: &'a str,
    pub(crate) yaml_start: usize,
    pub(crate) body_start: usize,
}

impl Frontmatter<'_> {
    /// The YAML string value of a top-level `key:` line, quotes decoded.
    pub(crate) fn scalar(self, key: &str) -> Option<String> {
        let raw = self
            .yaml
            .lines()
            .filter(|line| !line.starts_with(char::is_whitespace))
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))?
            .trim();
        serde_norway::from_str(raw).ok()
    }
}

/// Split the frontmatter from a note. Each delimiter is a whole `---` line
/// ended by LF or CRLF; a final unterminated line is not a delimiter.
pub fn split_frontmatter(contents: &str) -> Option<Frontmatter<'_>> {
    let (opening, yaml_start) = delimiter_line(contents, 0)?;
    if opening != "---" {
        return None;
    }
    let mut cursor = yaml_start;
    while let Some((line, next)) = delimiter_line(contents, cursor) {
        if line == "---" {
            return Some(Frontmatter {
                yaml: contents.get(yaml_start..cursor)?,
                yaml_start,
                body_start: next,
            });
        }
        cursor = next;
    }
    None
}

/// The line without its LF/CRLF terminator, plus the next byte offset.
fn delimiter_line(contents: &str, start: usize) -> Option<(&str, usize)> {
    let tail = contents.get(start..)?;
    let newline = tail.find('\n')?.checked_add(start)?;
    let raw_line = contents.get(start..newline)?;
    let line_end = raw_line
        .strip_suffix('\r')
        .map_or(newline, |_| newline.saturating_sub(1));
    let next = newline.checked_add(1)?;
    Some((contents.get(start..line_end)?, next))
}

pub fn markdown_headings(markdown: &str) -> Vec<(usize, String, usize)> {
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

fn in_ranges(byte_offset: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| byte_offset >= range.start && byte_offset < range.end)
}

/// The zero-based line of a byte offset: the newlines before it.
pub fn line_index(text: &str, byte_offset: usize) -> usize {
    text.get(..byte_offset.min(text.len()))
        .map_or(0, |prefix| prefix.matches('\n').count())
}

/// The one-based line of a byte offset.
pub fn line_number(text: &str, byte_offset: usize) -> usize {
    line_index(text, byte_offset).saturating_add(1)
}

const fn heading_level(level: HeadingLevel) -> usize {
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
    fn extracts_markdown_headings() {
        let headings = markdown_headings("# One\ntext\n### Three ###\nnot heading");
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].0, 1);
        assert_eq!(headings[0].1, "One");
        assert_eq!(headings[1].2, 3);
    }

    #[test]
    fn splits_exact_frontmatter_delimiters_with_lf_and_crlf() {
        for contents in [
            "---\nid: doc\n---\n# Body\n",
            "---\r\nid: doc\r\n---\r\n# Body\r\n",
            "---\r\nid: doc\n---\r\n# Body\n",
        ] {
            let frontmatter = split_frontmatter(contents).unwrap();
            assert!(frontmatter.yaml.contains("id: doc"));
            assert_eq!(frontmatter.scalar("id").as_deref(), Some("doc"));
            assert!(
                contents
                    .get(frontmatter.body_start..)
                    .is_some_and(|body| body.starts_with("# Body"))
            );
            assert_eq!(
                line_index(contents, frontmatter.body_start),
                3,
                "both delimiters and the field line"
            );
        }
    }

    #[test]
    fn frontmatter_scalars_decode_yaml_quoting() {
        let frontmatter = split_frontmatter(
            "---\nid: \"ADR-0001\"\ntitle: 'It''s here'\nstatus: accepted\n---\n",
        )
        .unwrap();

        assert_eq!(frontmatter.scalar("id").as_deref(), Some("ADR-0001"));
        assert_eq!(frontmatter.scalar("title").as_deref(), Some("It's here"));
        assert_eq!(frontmatter.scalar("status").as_deref(), Some("accepted"));
        assert_eq!(frontmatter.scalar("governs"), None);
    }

    #[test]
    fn ignores_delimiter_like_or_unclosed_frontmatter() {
        for contents in [
            "---\nid: doc\n---suffix\n# Body\n",
            "---\nid: doc\n# Body\n",
            "\u{feff}---\nid: doc\n---\n# Body\n",
            "---\nid: doc\n---",
        ] {
            assert_eq!(split_frontmatter(contents), None);
        }
    }

    proptest::proptest! {
        #[test]
        fn frontmatter_split_is_lossless(
            contents in r"(?s)(---|-|\r|\n|[a-z: ]|é){0,120}"
        ) {
            if let Some(frontmatter) = split_frontmatter(&contents) {
                let yaml_end = frontmatter.yaml_start.checked_add(frontmatter.yaml.len());
                proptest::prop_assert_eq!(
                    yaml_end.and_then(|end| contents.get(frontmatter.yaml_start..end)),
                    Some(frontmatter.yaml)
                );
                proptest::prop_assert!(contents.get(frontmatter.body_start..).is_some());
                proptest::prop_assert!(contents.starts_with("---\n") || contents.starts_with("---\r\n"));
            }
        }
    }

    #[test]
    fn supports_empty_frontmatter() {
        assert_eq!(
            split_frontmatter("---\n---\nbody\n"),
            Some(Frontmatter {
                yaml: "",
                yaml_start: 4,
                body_start: 8,
            })
        );
    }

    #[test]
    fn link_targets_separate_alias_fragment_and_typed_forms() {
        let note = LinkTarget::parse(" guide#Setup | The guide ");
        assert_eq!(note.as_str(), "guide#Setup");
        assert_eq!((note.path(), note.fragment()), ("guide", Some("Setup")));
        assert!(!note.is_source());

        let source = LinkTarget::parse("source:src/lib.rs#fn:run|run");
        assert!(source.is_source());
        assert_eq!(source.source_body(), "src/lib.rs#fn:run");
        assert_eq!(
            (source.path(), source.symbol()),
            ("src/lib.rs", Some("fn:run"))
        );

        let lines = LinkTarget::parse("src/lib.rs#L3-L9");
        assert_eq!(lines.fragment(), Some("L3-L9"));
        assert_eq!(lines.symbol(), None);

        assert_eq!(
            LinkTarget::parse("match:ADR-0001/no-print").pattern_id(),
            Some("ADR-0001/no-print")
        );
        assert_eq!(
            LinkTarget::parse("ADR-0001#match:ADR-0001/no-print").pattern_id(),
            Some("ADR-0001/no-print")
        );
    }

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

    proptest::proptest! {
        #[test]
        fn wiki_link_ranges_slice_back_to_their_links(
            markdown in r"(?s)([a-z\[\]`|#\n\r ]|é|中|\u{1F600}){0,200}"
        ) {
            for (line, link, range) in find_wiki_links_with_lines(&markdown) {
                let expected = format!("[[{link}]]");
                proptest::prop_assert_eq!(markdown.get(range.clone()), Some(expected.as_str()));
                let newlines_before = markdown.get(..range.start).map(|prefix| prefix.matches('\n').count());
                proptest::prop_assert_eq!(Some(line), newlines_before.and_then(|count| count.checked_add(1)));
            }
        }
    }
}

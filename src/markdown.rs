use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

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
}

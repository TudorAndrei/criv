//! Tree-sitter and delimiter helpers shared by every Source extractor.

use tree_sitter::{Node, TreeCursor};

use super::SymbolRange;

const MAX_AST_DEPTH: usize = 512;

/// The nesting level of a node below the root a walk started from.
#[derive(Debug, Clone, Copy)]
pub(super) struct Depth(usize);

impl Depth {
    pub(super) const ROOT: Self = Self(0);

    /// The depth of a child node, or `None` past the bound every walk shares.
    pub(super) fn child(self) -> Option<Self> {
        let next = self.0.saturating_add(1);
        (next < MAX_AST_DEPTH).then_some(Self(next))
    }
}

/// `node` and its descendants in document order, down to the shared depth
/// bound.
pub(super) fn descendants(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    Descendants {
        cursor: node.walk(),
        depth: Depth::ROOT,
        done: false,
    }
}

struct Descendants<'tree> {
    cursor: TreeCursor<'tree>,
    depth: Depth,
    done: bool,
}

impl<'tree> Iterator for Descendants<'tree> {
    type Item = Node<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let node = self.cursor.node();
        if let Some(child) = self.depth.child()
            && self.cursor.goto_first_child()
        {
            self.depth = child;
            return Some(node);
        }
        loop {
            if self.depth.0 == 0 {
                self.done = true;
                break;
            }
            if self.cursor.goto_next_sibling() {
                break;
            }
            self.cursor.goto_parent();
            self.depth = Depth(self.depth.0.saturating_sub(1));
        }
        Some(node)
    }
}

pub(super) fn node_text(node: Node<'_>, contents: &str) -> Option<String> {
    node.utf8_text(contents.as_bytes()).ok().map(str::to_string)
}

pub(super) fn field_text(node: Node<'_>, contents: &str, field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|child| node_text(child, contents))
}

pub(super) fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

pub(super) fn node_range(node: Node<'_>) -> SymbolRange {
    SymbolRange {
        start_line: node.start_position().row.saturating_add(1),
        end_line: node.end_position().row.saturating_add(1),
    }
}

/// The byte offset of the delimiter that closes the one at `open`.
pub(super) fn matching_close(
    value: &str,
    open: usize,
    open_char: char,
    close_char: char,
) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in value.get(open..)?.char_indices() {
        if character == open_char {
            depth = depth.saturating_add(1);
        } else if character == close_char {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return open.checked_add(offset);
            }
        }
    }
    None
}

/// The non-empty trimmed parts of `value` between `separator`s outside
/// `()`, `[]`, `{}`, and `<>`.
pub(super) fn split_top_level(value: &str, separator: char) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut depths = Delimiters::default();
    let mut start = 0usize;
    for (index, character) in value.char_indices() {
        depths.update(character);
        if character == separator && depths.at_top_level() {
            if let Some(row) = value.get(start..index).map(str::trim)
                && !row.is_empty()
            {
                rows.push(row);
            }
            start = index.saturating_add(character.len_utf8());
        }
    }
    if let Some(row) = value.get(start..).map(str::trim)
        && !row.is_empty()
    {
        rows.push(row);
    }
    rows
}

/// `value` split at the first `operator` outside `()`, `[]`, `{}`, and `<>`.
pub(super) fn split_top_level_operator<'a>(
    value: &'a str,
    operator: &str,
) -> Option<(&'a str, &'a str)> {
    let mut depths = Delimiters::default();
    for (index, character) in value.char_indices() {
        depths.update(character);
        if depths.at_top_level()
            && let Some(tail) = value.get(index..)
            && tail.starts_with(operator)
            && let Some(right_start) = index.checked_add(operator.len())
            && let (Some(left), Some(right)) = (value.get(..index), value.get(right_start..))
        {
            return Some((left, right));
        }
    }
    None
}

#[derive(Default)]
struct Delimiters([usize; 4]);

impl Delimiters {
    const fn update(&mut self, character: char) {
        let [parentheses, brackets, braces, angles] = &mut self.0;
        match character {
            '(' => *parentheses = parentheses.saturating_add(1),
            ')' => *parentheses = parentheses.saturating_sub(1),
            '[' => *brackets = brackets.saturating_add(1),
            ']' => *brackets = brackets.saturating_sub(1),
            '{' => *braces = braces.saturating_add(1),
            '}' => *braces = braces.saturating_sub(1),
            '<' => *angles = angles.saturating_add(1),
            '>' => *angles = angles.saturating_sub(1),
            _ => {}
        }
    }

    fn at_top_level(&self) -> bool {
        self.0 == [0; 4]
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter::Parser;

    use super::*;

    #[test]
    fn descendants_stop_at_the_shared_depth_bound() {
        let nested = format!("{}1{}", "(".repeat(600), ")".repeat(600));
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&nested, None).unwrap();

        let deepest = descendants(tree.root_node())
            .map(|node| {
                let mut depth = 0usize;
                let mut current = node;
                while let Some(parent) = current.parent() {
                    depth += 1;
                    current = parent;
                }
                depth
            })
            .max()
            .unwrap();

        assert_eq!(deepest, MAX_AST_DEPTH - 1);
    }
}

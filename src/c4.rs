use std::collections::BTreeSet;

use crate::util::markdown_fenced_blocks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum C4Level {
    Context,
    Container,
    Component,
}

impl C4Level {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Container => "container",
            Self::Component => "component",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4Element {
    pub(crate) alias: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) technology: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) duplicate_source_lines: Vec<usize>,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4Relationship {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) label: Option<String>,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4Diagram {
    pub(crate) level: C4Level,
    pub(crate) elements: Vec<C4Element>,
    pub(crate) relationships: Vec<C4Relationship>,
    pub(crate) line: usize,
}

impl C4Diagram {
    pub(crate) fn duplicate_aliases(&self) -> Vec<(String, usize)> {
        let mut seen = BTreeSet::new();
        let mut duplicates = Vec::new();
        for element in &self.elements {
            if !seen.insert(element.alias.as_str()) {
                duplicates.push((element.alias.clone(), element.line));
            }
        }
        duplicates
    }

    pub(crate) fn duplicate_sources(&self) -> Vec<(&C4Element, usize)> {
        self.elements
            .iter()
            .flat_map(|element| {
                element
                    .duplicate_source_lines
                    .iter()
                    .map(move |line| (element, *line))
            })
            .collect()
    }

    pub(crate) fn unresolved_relationships(&self) -> Vec<&C4Relationship> {
        let aliases = self
            .elements
            .iter()
            .map(|element| element.alias.as_str())
            .collect::<BTreeSet<_>>();
        self.relationships
            .iter()
            .filter(|relationship| {
                !aliases.contains(relationship.from.as_str())
                    || !aliases.contains(relationship.to.as_str())
            })
            .collect()
    }

}

pub(crate) fn parse_diagrams(markdown: &str) -> Vec<C4Diagram> {
    markdown_fenced_blocks(markdown)
        .into_iter()
        .filter(|(_, info, _)| info.as_deref() == Some("mermaid"))
        .filter_map(|(start_line, _, contents)| parse_diagram(start_line, &contents))
        .collect()
}

fn parse_diagram(start_line: usize, contents: &str) -> Option<C4Diagram> {
    let mut lines = contents.lines().enumerate();
    let (header_index, header) = lines.find_map(|(index, line)| {
        let trimmed = line.trim();
        (!trimmed.is_empty()).then_some((index, trimmed))
    })?;
    let level = match header {
        "C4Context" => C4Level::Context,
        "C4Container" => C4Level::Container,
        "C4Component" => C4Level::Component,
        _ => return None,
    };

    let mut diagram = C4Diagram {
        level,
        elements: Vec::new(),
        relationships: Vec::new(),
        line: start_line + header_index + 1,
    };
    let mut last_element: Option<usize> = None;

    for (index, line) in lines {
        let line_number = start_line + index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            last_element = None;
            continue;
        }
        if let Some(source) = source_comment(trimmed) {
            if let Some(element_index) = last_element {
                let element = &mut diagram.elements[element_index];
                if element.source.is_some() {
                    element.duplicate_source_lines.push(line_number);
                } else {
                    element.source = Some(source);
                }
            }
            continue;
        }
        if trimmed.starts_with("%%") {
            last_element = None;
            continue;
        }

        let Some((name, args)) = parse_macro(trimmed) else {
            last_element = None;
            continue;
        };

        if is_relationship_macro(&name) {
            if let Some(relationship) = parse_relationship(args, line_number) {
                diagram.relationships.push(relationship);
            }
            last_element = None;
        } else if is_element_macro(&name) {
            if let Some(element) = parse_element(name, args, line_number) {
                diagram.elements.push(element);
                last_element = Some(diagram.elements.len() - 1);
            } else {
                last_element = None;
            }
        } else {
            last_element = None;
        }
    }

    Some(diagram)
}

fn source_comment(line: &str) -> Option<String> {
    line.strip_prefix("%%")
        .map(str::trim)
        .and_then(|comment| comment.strip_prefix("criv:source"))
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_string)
}

fn parse_macro(line: &str) -> Option<(String, Vec<String>)> {
    let open = line.find('(')?;
    let name = line[..open].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    let close = matching_paren(line, open)?;
    let args = split_args(&line[open + 1..close]);
    Some((name.to_string(), args))
}

fn matching_paren(line: &str, open: usize) -> Option<usize> {
    let mut in_quote = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for (index, ch) in line.char_indices().skip_while(|(index, _)| *index < open) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quote => escaped = true,
            '"' => in_quote = !in_quote,
            '(' if !in_quote => depth += 1,
            ')' if !in_quote => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_args(args: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escaped = false;

    for ch in args.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quote => escaped = true,
            '"' => in_quote = !in_quote,
            ',' if !in_quote => {
                values.push(clean_arg(&current));
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() || args.ends_with(',') {
        values.push(clean_arg(&current));
    }

    values
}

fn clean_arg(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

fn is_relationship_macro(name: &str) -> bool {
    name == "BiRel" || name.starts_with("Rel")
}

fn is_element_macro(name: &str) -> bool {
    matches!(
        name,
        "Boundary" | "Enterprise_Boundary" | "System_Boundary" | "Container_Boundary"
    ) || name.ends_with("_Boundary")
        || name.starts_with("Person")
        || name.starts_with("System")
        || name.starts_with("Container")
        || name.starts_with("Component")
}

fn parse_element(kind: String, args: Vec<String>, line: usize) -> Option<C4Element> {
    let alias = args.first()?.trim().to_string();
    if alias.is_empty() {
        return None;
    }
    Some(C4Element {
        alias,
        kind,
        label: args.get(1).cloned().unwrap_or_default(),
        technology: args.get(2).cloned().filter(|value| !value.is_empty()),
        description: args.get(3).cloned().filter(|value| !value.is_empty()),
        source: None,
        duplicate_source_lines: Vec::new(),
        line,
    })
}

fn parse_relationship(args: Vec<String>, line: usize) -> Option<C4Relationship> {
    Some(C4Relationship {
        from: args.first()?.to_string(),
        to: args.get(1)?.to_string(),
        label: args.get(2).cloned().filter(|value| !value.is_empty()),
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_container_elements_and_relationships() {
        let diagrams = parse_diagrams(
            r#"
```mermaid
C4Container
    System_Boundary(c1, "criv") {
        Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
        Container(plugin, "Obsidian Plugin", "TypeScript/WASM", "Reads generated state")
        Rel(cli, plugin, "writes state for")
    }
```
"#,
        );

        assert_eq!(diagrams.len(), 1);
        let diagram = &diagrams[0];
        assert_eq!(diagram.level, C4Level::Container);
        assert_eq!(diagram.elements.len(), 3);
        assert_eq!(diagram.elements[0].alias, "c1");
        assert_eq!(diagram.elements[1].kind, "Container");
        assert_eq!(diagram.elements[1].label, "criv CLI");
        assert_eq!(diagram.relationships.len(), 1);
        assert_eq!(diagram.relationships[0].from, "cli");
        assert_eq!(diagram.relationships[0].to, "plugin");
    }

    #[test]
    fn source_comment_annotates_preceding_element() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs
```
"#,
        )
        .remove(0);

        assert_eq!(diagram.elements[0].source.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn repeated_source_comments_are_tracked() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs
%% criv:source src/lib.rs
```
"#,
        )
        .remove(0);

        assert_eq!(diagram.elements[0].source.as_deref(), Some("src/main.rs"));
        assert_eq!(diagram.duplicate_sources().len(), 1);
    }

    #[test]
    fn ignores_non_c4_mermaid_blocks() {
        let diagrams = parse_diagrams(
            r#"
```mermaid
flowchart TD
    a --> b
```
"#,
        );

        assert!(diagrams.is_empty());
    }

    #[test]
    fn ignores_style_and_unknown_macros() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
UpdateElementStyle(cli, $bgColor="red")
Contianer(oops, "typo")
```
"#,
        )
        .remove(0);

        assert_eq!(diagram.elements.len(), 1);
        assert_eq!(diagram.elements[0].alias, "cli");
    }

    #[test]
    fn duplicate_aliases_are_detected() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
Container(cli, "Other CLI", "Rust", "Duplicates alias")
```
"#,
        )
        .remove(0);

        assert_eq!(diagram.duplicate_aliases()[0].0, "cli");
    }

    #[test]
    fn unresolved_relationships_are_detected() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
Rel(cli, plugin, "writes state for")
```
"#,
        )
        .remove(0);

        assert_eq!(diagram.unresolved_relationships()[0].to, "plugin");
    }
}

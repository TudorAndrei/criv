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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum C4ElementCategory {
    Person,
    SoftwareSystem,
    Container,
    Component,
}

impl C4ElementCategory {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::SoftwareSystem => "software-system",
            Self::Container => "container",
            Self::Component => "component",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4Element {
    pub(crate) alias: String,
    pub(crate) kind: String,
    pub(crate) category: C4ElementCategory,
    pub(crate) label: String,
    pub(crate) technology: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) source: Option<String>,
    duplicate_source_lines: Vec<usize>,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4Boundary {
    alias: String,
    kind: String,
    label: String,
    line: usize,
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
    boundaries: Vec<C4Boundary>,
    pub(crate) relationships: Vec<C4Relationship>,
    pub(crate) invalid_source_placements: Vec<(usize, String)>,
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

/// `line_offset` is the number of lines that precede `markdown` in the file it
/// came from, so the reported diagram lines stay file-relative.
pub(crate) fn parse_diagrams(markdown: &str, line_offset: usize) -> Vec<C4Diagram> {
    markdown_fenced_blocks(markdown)
        .into_iter()
        .filter(|(_, info, _)| info.as_deref() == Some("mermaid"))
        .filter_map(|(start_line, _, contents)| {
            parse_mermaid_diagram(start_line + line_offset, &contents)
        })
        .collect()
}

pub(crate) fn parse_mermaid_diagram(start_line: usize, contents: &str) -> Option<C4Diagram> {
    let mut lines = contents.lines().enumerate();
    let (header_index, header) = lines.find_map(|(index, line)| {
        let trimmed = line.trim();
        (!trimmed.is_empty() && !trimmed.starts_with("%%")).then_some((index, trimmed))
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
        boundaries: Vec::new(),
        relationships: Vec::new(),
        invalid_source_placements: Vec::new(),
        line: start_line + header_index + 1,
    };
    let mut last_construct = LastConstruct::None;

    for (index, line) in lines {
        let line_number = start_line + index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            last_construct = LastConstruct::None;
            continue;
        }
        if let Some(source) = source_comment(trimmed) {
            if let LastConstruct::Element(element_index) = last_construct {
                let element = &mut diagram.elements[element_index];
                if element.source.is_some() {
                    element.duplicate_source_lines.push(line_number);
                } else {
                    element.source = Some(source);
                }
            } else {
                diagram
                    .invalid_source_placements
                    .push((line_number, source));
            }
            continue;
        }
        if trimmed.starts_with("%%") {
            last_construct = LastConstruct::None;
            continue;
        }

        let Some((name, args)) = parse_macro(trimmed) else {
            last_construct = LastConstruct::None;
            continue;
        };

        if is_relationship_macro(&name) {
            if let Some(relationship) = parse_relationship(args, line_number) {
                diagram.relationships.push(relationship);
            }
            last_construct = LastConstruct::Relationship;
        } else if is_boundary_macro(&name) {
            if let Some(boundary) = parse_boundary(name, args, line_number) {
                diagram.boundaries.push(boundary);
                last_construct = LastConstruct::Boundary;
            } else {
                last_construct = LastConstruct::None;
            }
        } else if let Some(category) = element_category(&name) {
            if let Some(element) = parse_element(name, category, args, line_number) {
                diagram.elements.push(element);
                last_construct = LastConstruct::Element(diagram.elements.len() - 1);
            } else {
                last_construct = LastConstruct::None;
            }
        } else {
            last_construct = LastConstruct::None;
        }
    }

    Some(diagram)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastConstruct {
    None,
    Element(usize),
    Boundary,
    Relationship,
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

fn is_boundary_macro(name: &str) -> bool {
    matches!(
        name,
        "Boundary" | "Enterprise_Boundary" | "System_Boundary" | "Container_Boundary"
    ) || name.ends_with("_Boundary")
}

fn element_category(name: &str) -> Option<C4ElementCategory> {
    if name.starts_with("Person") {
        Some(C4ElementCategory::Person)
    } else if name.starts_with("System") {
        Some(C4ElementCategory::SoftwareSystem)
    } else if name.starts_with("Container") {
        Some(C4ElementCategory::Container)
    } else if name.starts_with("Component") {
        Some(C4ElementCategory::Component)
    } else {
        None
    }
}

fn parse_element(
    kind: String,
    category: C4ElementCategory,
    args: Vec<String>,
    line: usize,
) -> Option<C4Element> {
    let alias = args.first()?.trim().to_string();
    if alias.is_empty() {
        return None;
    }
    Some(C4Element {
        alias,
        kind,
        category,
        label: args.get(1).cloned().unwrap_or_default(),
        technology: match category {
            C4ElementCategory::Person | C4ElementCategory::SoftwareSystem => None,
            C4ElementCategory::Container | C4ElementCategory::Component => {
                args.get(2).cloned().filter(|value| !value.is_empty())
            }
        },
        description: match category {
            C4ElementCategory::Person | C4ElementCategory::SoftwareSystem => {
                args.get(2).cloned().filter(|value| !value.is_empty())
            }
            C4ElementCategory::Container | C4ElementCategory::Component => {
                args.get(3).cloned().filter(|value| !value.is_empty())
            }
        },
        source: None,
        duplicate_source_lines: Vec::new(),
        line,
    })
}

fn parse_boundary(kind: String, args: Vec<String>, line: usize) -> Option<C4Boundary> {
    let alias = args.first()?.trim().to_string();
    if alias.is_empty() {
        return None;
    }
    Some(C4Boundary {
        alias,
        kind,
        label: args.get(1).cloned().unwrap_or_default(),
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
            0,
        );

        assert_eq!(diagrams.len(), 1);
        let diagram = &diagrams[0];
        assert_eq!(diagram.level, C4Level::Container);
        assert_eq!(diagram.boundaries.len(), 1);
        assert_eq!(diagram.boundaries[0].alias, "c1");
        assert_eq!(diagram.boundaries[0].kind, "System_Boundary");
        assert_eq!(diagram.elements.len(), 2);
        assert_eq!(diagram.elements[0].kind, "Container");
        assert_eq!(diagram.elements[0].category, C4ElementCategory::Container);
        assert_eq!(diagram.elements[0].label, "criv CLI");
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
            0,
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
            0,
        )
        .remove(0);

        assert_eq!(diagram.elements[0].source.as_deref(), Some("src/main.rs"));
        assert_eq!(diagram.duplicate_sources().len(), 1);
    }

    #[test]
    fn normalizes_external_person_and_system_variants() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Context
Person_Ext(user, "Repository maintainer", "Uses the CLI")
System_Ext(github, "GitHub", "Renders Mermaid diagrams")
```
"#,
            0,
        )
        .remove(0);

        assert_eq!(diagram.elements.len(), 2);
        assert_eq!(diagram.elements[0].kind, "Person_Ext");
        assert_eq!(diagram.elements[0].category, C4ElementCategory::Person);
        assert_eq!(diagram.elements[1].kind, "System_Ext");
        assert_eq!(
            diagram.elements[1].category,
            C4ElementCategory::SoftwareSystem
        );
        assert_eq!(
            diagram.elements[1].description.as_deref(),
            Some("Renders Mermaid diagrams")
        );
        assert_eq!(diagram.elements[1].technology, None);
    }

    #[test]
    fn source_comment_after_boundary_is_invalid_placement() {
        let diagram = parse_diagrams(
            r#"
```mermaid
C4Container
System_Boundary(system, "criv") {
%% criv:source src/main.rs
}
```
"#,
            0,
        )
        .remove(0);

        assert_eq!(diagram.boundaries.len(), 1);
        assert!(diagram.elements.is_empty());
        assert_eq!(
            diagram.invalid_source_placements,
            vec![(5, "src/main.rs".into())]
        );
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
            0,
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
            0,
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
            0,
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
            0,
        )
        .remove(0);

        assert_eq!(diagram.unresolved_relationships()[0].to, "plugin");
    }
}

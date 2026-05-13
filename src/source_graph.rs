use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use tree_sitter::{Node, Parser};

use crate::Result;
use crate::util::read_to_string;

#[derive(Debug, Default, Clone)]
pub(crate) struct SourceGraph {
    pub(crate) files: BTreeMap<String, SourceFile>,
    file_fingerprints: BTreeMap<String, String>,
    changed_files: Vec<String>,
    symbol_index: BTreeMap<String, Vec<SymbolId>>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SourceFile {
    pub(crate) path: String,
    pub(crate) language: Language,
    pub(crate) imports: Vec<Import>,
    pub(crate) symbols: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub(crate) struct Import {
    pub(crate) module: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Symbol {
    pub(crate) id: SymbolId,
    pub(crate) name: String,
    pub(crate) kind: SymbolKind,
    pub(crate) parent: Option<String>,
    pub(crate) exported: bool,
    pub(crate) range: SymbolRange,
    pub(crate) calls: Vec<Call>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct SymbolRange {
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct SymbolId {
    pub(crate) path: String,
    pub(crate) name: String,
}

impl SymbolId {
    pub(crate) fn display(&self) -> String {
        format!("{}#{}", self.path, self.name)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub(crate) target: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SymbolKind {
    Function,
    Method,
    Class,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub(crate) enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    #[default]
    Unknown,
}

impl SourceGraph {
    pub(crate) fn build_incremental(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        let mut graph = Self::default();
        for source_file in source_files {
            let path = root.join(source_file);
            let fingerprint = source_file_fingerprint(&path)?;
            let reused = previous
                .filter(|previous| {
                    previous.file_fingerprints.get(source_file) == Some(&fingerprint)
                })
                .and_then(|previous| previous.files.get(source_file).cloned());
            let parsed = if let Some(parsed) = reused {
                parsed
            } else {
                graph.changed_files.push(source_file.clone());
                let contents = read_to_string(&path)?;
                parse_source_file(source_file, &contents)
            };
            for symbol in &parsed.symbols {
                graph
                    .symbol_index
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(symbol.id.clone());
            }
            graph
                .file_fingerprints
                .insert(source_file.clone(), fingerprint);
            graph.files.insert(source_file.clone(), parsed);
        }
        if let Some(previous) = previous {
            for previous_file in previous.file_fingerprints.keys() {
                if !graph.file_fingerprints.contains_key(previous_file) {
                    graph.changed_files.push(previous_file.clone());
                }
            }
            graph.changed_files.sort();
            graph.changed_files.dedup();
        }
        Ok(graph)
    }

    pub(crate) fn resolve_symbol(&self, query: &str) -> Option<SymbolId> {
        let (path, name) = query.split_once('#').unwrap_or(("", query));
        if !path.is_empty() {
            return self
                .files
                .get(path)
                .and_then(|file| file.symbols.iter().find(|symbol| symbol.name == name))
                .map(|symbol| symbol.id.clone());
        }

        self.symbol_index
            .get(name)
            .and_then(|matches| matches.first().cloned())
    }

    pub(crate) fn callees(&self, query: &str) -> Vec<String> {
        let Some(symbol_id) = self.resolve_symbol(query) else {
            return Vec::new();
        };
        let mut rows = self
            .symbol(&symbol_id)
            .map(|symbol| {
                symbol
                    .calls
                    .iter()
                    .map(|call| {
                        self.resolve_call(&symbol.id, &call.target)
                            .map_or_else(|| call.target.clone(), |id| id.display())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        rows.sort();
        rows.dedup();
        rows
    }

    pub(crate) fn callers(&self, query: &str) -> Vec<String> {
        let Some(symbol_id) = self.resolve_symbol(query) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for file in self.files.values() {
            for symbol in &file.symbols {
                if symbol.calls.iter().any(|call| {
                    self.resolve_call(&symbol.id, &call.target)
                        .is_some_and(|target| target == symbol_id)
                        || call.target == symbol_id.name
                }) {
                    rows.push(symbol.id.display());
                }
            }
        }
        rows.sort();
        rows.dedup();
        rows
    }

    pub(crate) fn attack_surface(&self) -> Vec<String> {
        let mut called = BTreeSet::new();
        for file in self.files.values() {
            for symbol in &file.symbols {
                for call in &symbol.calls {
                    if let Some(target) = self.resolve_call(&symbol.id, &call.target) {
                        called.insert(target);
                    }
                }
            }
        }

        let mut rows = self
            .files
            .values()
            .flat_map(|file| &file.symbols)
            .filter(|symbol| {
                symbol.exported
                    || symbol.name == "main"
                    || (symbol.parent.is_none() && !called.contains(&symbol.id))
            })
            .map(|symbol| symbol.id.display())
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }

    pub(crate) fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.files.values().flat_map(|file| file.symbols.iter())
    }

    pub(crate) fn changed_files(&self) -> &[String] {
        &self.changed_files
    }

    pub(crate) fn resolve_call(&self, caller: &SymbolId, target: &str) -> Option<SymbolId> {
        self.files
            .get(&caller.path)
            .and_then(|file| file.symbols.iter().find(|symbol| symbol.name == target))
            .map(|symbol| symbol.id.clone())
            .or_else(|| self.resolve_symbol(target))
    }

    fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.files
            .get(&id.path)
            .and_then(|file| file.symbols.iter().find(|symbol| &symbol.id == id))
    }
}

fn source_file_fingerprint(path: &Path) -> Result<String> {
    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(format!("{}\0{modified}", metadata.len()))
}

fn parse_source_file(path: &str, contents: &str) -> SourceFile {
    parse_tree_sitter_file(path, contents)
        .unwrap_or_else(|| parse_source_file_fallback(path, contents))
}

fn parse_tree_sitter_file(path: &str, contents: &str) -> Option<SourceFile> {
    let language = Language::from_path(path);
    let tree_sitter_language = tree_sitter_language(language)?;
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_language).ok()?;
    let tree = parser.parse(contents, None)?;
    if tree.root_node().has_error() {
        return None;
    }

    let mut file = SourceFile {
        path: path.into(),
        language,
        imports: Vec::new(),
        symbols: Vec::new(),
    };
    collect_tree_sitter_nodes(tree.root_node(), contents, path, language, None, &mut file);
    Some(file)
}

fn tree_sitter_language(language: Language) -> Option<tree_sitter::Language> {
    match language {
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Language::Unknown => None,
    }
}

fn collect_tree_sitter_nodes(
    node: Node<'_>,
    contents: &str,
    path: &str,
    language: Language,
    parent: Option<String>,
    file: &mut SourceFile,
) {
    if let Some(import) = tree_sitter_import(node, contents, language) {
        file.imports.push(Import {
            module: import,
            line: node.start_position().row + 1,
        });
    }

    if let Some(symbol) = tree_sitter_symbol(node, contents, path, language, parent.as_deref()) {
        let symbol_parent = if symbol.kind == SymbolKind::Class {
            Some(symbol.name.clone())
        } else {
            parent.clone()
        };
        file.symbols.push(symbol);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_tree_sitter_nodes(child, contents, path, language, symbol_parent.clone(), file);
        }
        return;
    }

    let impl_parent = if language == Language::Rust && node.kind() == "impl_item" {
        node_text(node, contents).and_then(|text| parse_rust_impl_target(&text))
    } else {
        parent
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_nodes(child, contents, path, language, impl_parent.clone(), file);
    }
}

fn tree_sitter_import(node: Node<'_>, contents: &str, language: Language) -> Option<String> {
    match (language, node.kind()) {
        (Language::Rust, "use_declaration") => node_text(node, contents).map(|text| {
            text.trim()
                .trim_start_matches("use ")
                .trim_start_matches("pub use ")
                .trim_end_matches(';')
                .trim()
                .to_string()
        }),
        (Language::TypeScript | Language::JavaScript, "import_statement") => {
            descendant_of_kind(node, "string")
                .and_then(|child| node_text(child, contents))
                .map(|text| clean_js_module(&text))
        }
        (Language::Python, "import_statement" | "import_from_statement") => {
            node_text(node, contents).and_then(|text| parse_import(text.trim(), language))
        }
        (Language::Go, "import_spec") => descendant_of_kind(node, "interpreted_string_literal")
            .or_else(|| descendant_of_kind(node, "raw_string_literal"))
            .and_then(|child| node_text(child, contents))
            .map(|text| text.trim_matches('"').trim_matches('`').to_string()),
        _ => None,
    }
}

fn tree_sitter_symbol(
    node: Node<'_>,
    contents: &str,
    path: &str,
    language: Language,
    parent: Option<&str>,
) -> Option<Symbol> {
    let (name, kind) = match (language, node.kind()) {
        (Language::Rust, "function_item") => (
            field_text(node, contents, "name")?,
            if parent.is_some() {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            },
        ),
        (Language::Rust, "struct_item" | "enum_item") => {
            (field_text(node, contents, "name")?, SymbolKind::Class)
        }
        (Language::TypeScript | Language::JavaScript, "function_declaration")
        | (Language::TypeScript | Language::JavaScript, "generator_function_declaration") => {
            (field_text(node, contents, "name")?, SymbolKind::Function)
        }
        (Language::TypeScript | Language::JavaScript, "method_definition")
        | (Language::TypeScript | Language::JavaScript, "method_signature") => {
            (field_text(node, contents, "name")?, SymbolKind::Method)
        }
        (Language::TypeScript | Language::JavaScript, "class_declaration") => {
            (field_text(node, contents, "name")?, SymbolKind::Class)
        }
        (Language::TypeScript | Language::JavaScript, "variable_declarator")
            if has_descendant_kind(node, &["arrow_function", "function_expression"]) =>
        {
            (field_text(node, contents, "name")?, SymbolKind::Function)
        }
        (Language::Python, "function_definition") => (
            field_text(node, contents, "name")?,
            if parent.is_some() {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            },
        ),
        (Language::Python, "class_definition") => {
            (field_text(node, contents, "name")?, SymbolKind::Class)
        }
        (Language::Go, "function_declaration") => {
            (field_text(node, contents, "name")?, SymbolKind::Function)
        }
        (Language::Go, "method_declaration") => {
            (field_text(node, contents, "name")?, SymbolKind::Method)
        }
        (Language::Go, "type_spec") => (field_text(node, contents, "name")?, SymbolKind::Class),
        _ => return None,
    };

    Some(Symbol {
        id: SymbolId {
            path: path.into(),
            name: name.clone(),
        },
        name,
        kind,
        parent: parent.map(str::to_string),
        exported: tree_sitter_exported(node, contents, language),
        range: SymbolRange {
            start_line: node.start_position().row + 1,
            end_line: node.end_position().row + 1,
        },
        calls: tree_sitter_calls(node, contents, language),
    })
}

fn tree_sitter_calls(node: Node<'_>, contents: &str, language: Language) -> Vec<Call> {
    let mut calls = Vec::new();
    collect_tree_sitter_calls(node, contents, language, &mut calls);
    calls.sort_by(|left, right| (left.line, &left.target).cmp(&(right.line, &right.target)));
    calls.dedup_by(|left, right| left.line == right.line && left.target == right.target);
    calls
}

fn collect_tree_sitter_calls(
    node: Node<'_>,
    contents: &str,
    language: Language,
    calls: &mut Vec<Call>,
) {
    if node.kind() == "call_expression" || node.kind() == "call" {
        let function = node
            .child_by_field_name("function")
            .or_else(|| first_named_child(node));
        if let Some(target) = function.and_then(|child| call_target(child, contents)) {
            if !is_call_keyword(&target, language) {
                calls.push(Call {
                    target,
                    line: node.start_position().row + 1,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_calls(child, contents, language, calls);
    }
}

fn call_target(node: Node<'_>, contents: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "property_identifier" => node_text(node, contents),
        "selector_expression" | "field_expression" | "member_expression" | "attribute" => node
            .child_by_field_name("field")
            .or_else(|| node.child_by_field_name("property"))
            .or_else(|| node.child_by_field_name("attribute"))
            .and_then(|child| node_text(child, contents))
            .or_else(|| {
                node_text(node, contents).and_then(|text| {
                    text.rsplit(['.', ':'])
                        .next()
                        .map(str::trim)
                        .map(str::to_string)
                })
            }),
        _ => node_text(node, contents).and_then(|text| {
            text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .find(|part| !part.is_empty())
                .map(str::to_string)
        }),
    }
}

fn tree_sitter_exported(node: Node<'_>, contents: &str, language: Language) -> bool {
    match language {
        Language::Rust => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|child| child.kind() == "visibility_modifier")
        }
        Language::TypeScript | Language::JavaScript => {
            node.parent()
                .is_some_and(|parent| parent.kind() == "export_statement")
                || node_text(node, contents)
                    .is_some_and(|text| text.trim_start().starts_with("export "))
        }
        Language::Go => field_text(node, contents, "name")
            .is_some_and(|name| name.chars().next().is_some_and(char::is_uppercase)),
        Language::Python => {
            field_text(node, contents, "name").is_some_and(|name| !name.starts_with('_'))
        }
        Language::Unknown => false,
    }
}

fn field_text(node: Node<'_>, contents: &str, field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|child| node_text(child, contents))
}

fn node_text(node: Node<'_>, contents: &str) -> Option<String> {
    node.utf8_text(contents.as_bytes()).ok().map(str::to_string)
}

fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    (0..node.named_child_count()).find_map(|index| node.named_child(index as u32))
}

fn descendant_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = descendant_of_kind(child, kind) {
            return Some(found);
        }
    }
    None
}

fn has_descendant_kind(node: Node<'_>, kinds: &[&str]) -> bool {
    kinds
        .iter()
        .any(|kind| descendant_of_kind(node, kind).is_some())
}

fn parse_source_file_fallback(path: &str, contents: &str) -> SourceFile {
    let language = Language::from_path(path);
    let mut file = SourceFile {
        path: path.into(),
        language,
        imports: Vec::new(),
        symbols: Vec::new(),
    };

    let mut current_symbol: Option<usize> = None;
    let mut brace_depth = 0isize;
    let mut rust_impl_depth = 0isize;
    let mut rust_impl_target: Option<String> = None;
    let mut python_indent: Option<usize> = None;
    let mut python_class: Option<(String, usize)> = None;

    let total_lines = contents.lines().count().max(1);

    for (index, line) in contents.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, language) {
            continue;
        }

        if let Some(import) = parse_import(trimmed, language) {
            file.imports.push(Import {
                module: import,
                line: line_no,
            });
        }

        if language == Language::Python {
            let indent = line.len() - line.trim_start().len();
            if python_class.as_ref().is_some_and(|(_, class_indent)| {
                indent <= *class_indent && !trimmed.starts_with('@')
            }) {
                python_class = None;
            }
            if let Some(active_indent) = python_indent {
                if indent <= active_indent && !trimmed.starts_with('@') {
                    if let Some(symbol_index) = current_symbol {
                        file.symbols[symbol_index].range.end_line = line_no.saturating_sub(1);
                    }
                    current_symbol = None;
                    python_indent = None;
                }
            }
        }

        let in_rust_impl = language == Language::Rust && rust_impl_depth > 0;
        if let Some((name, mut kind)) = parse_symbol(trimmed, language, in_rust_impl) {
            let parent = if language == Language::Rust && kind == SymbolKind::Method {
                rust_impl_target.clone()
            } else if language == Language::Python && kind == SymbolKind::Function {
                let indent = line.len() - line.trim_start().len();
                python_class
                    .as_ref()
                    .filter(|(_, class_indent)| indent > *class_indent)
                    .map(|(class_name, _)| {
                        kind = SymbolKind::Method;
                        class_name.clone()
                    })
            } else {
                None
            };
            let id = SymbolId {
                path: path.into(),
                name: name.clone(),
            };
            file.symbols.push(Symbol {
                id,
                name,
                kind,
                parent,
                exported: is_exported_symbol(trimmed, language),
                range: SymbolRange {
                    start_line: line_no,
                    end_line: total_lines,
                },
                calls: Vec::new(),
            });
            current_symbol = Some(file.symbols.len() - 1);
            if language == Language::Python {
                python_indent = Some(line.len() - line.trim_start().len());
                if kind == SymbolKind::Class {
                    python_class = Some((
                        file.symbols.last().expect("pushed").name.clone(),
                        line.len() - line.trim_start().len(),
                    ));
                }
            }
        }

        if let Some(symbol_index) = current_symbol {
            let calls = parse_calls(trimmed, language)
                .into_iter()
                .filter(|call| call != &file.symbols[symbol_index].name)
                .map(|target| Call {
                    target,
                    line: line_no,
                })
                .collect::<Vec<_>>();
            file.symbols[symbol_index].calls.extend(calls);
        }

        if language != Language::Python {
            if language == Language::Rust && trimmed.starts_with("impl ") {
                rust_impl_target = parse_rust_impl_target(trimmed);
                rust_impl_depth += line.matches('{').count().max(1) as isize;
            } else if rust_impl_depth > 0 {
                rust_impl_depth += line.matches('{').count() as isize;
                rust_impl_depth -= line.matches('}').count() as isize;
                rust_impl_depth = rust_impl_depth.max(0);
                if rust_impl_depth == 0 {
                    rust_impl_target = None;
                }
            }
            brace_depth += line.matches('{').count() as isize;
            brace_depth -= line.matches('}').count() as isize;
            if brace_depth <= 0 {
                if let Some(symbol_index) = current_symbol {
                    file.symbols[symbol_index].range.end_line = line_no;
                }
                current_symbol = None;
                brace_depth = 0;
            }
        }
    }

    if let Some(symbol_index) = current_symbol {
        file.symbols[symbol_index].range.end_line = total_lines;
    }

    file
}

fn parse_import(line: &str, language: Language) -> Option<String> {
    match language {
        Language::Rust => line
            .strip_prefix("use ")
            .or_else(|| line.strip_prefix("pub use "))
            .map(|value| value.trim_end_matches(';').trim().to_string())
            .or_else(|| {
                line.strip_prefix("mod ")
                    .map(|value| value.trim_end_matches(';').trim().to_string())
            }),
        Language::TypeScript | Language::JavaScript => {
            if let Some((_, module)) = line.split_once(" from ") {
                Some(clean_js_module(module))
            } else {
                line.strip_prefix("import ").map(clean_js_module)
            }
        }
        Language::Python => line
            .strip_prefix("import ")
            .map(|value| value.split_whitespace().next().unwrap_or(value).to_string())
            .or_else(|| {
                line.strip_prefix("from ")
                    .and_then(|value| value.split_once(" import "))
                    .map(|(module, _)| module.to_string())
            }),
        Language::Go => line.strip_prefix("import ").map(|value| {
            value
                .trim()
                .trim_matches('"')
                .trim_matches('(')
                .trim_matches(')')
                .to_string()
        }),
        Language::Unknown => None,
    }
}

fn parse_symbol(
    line: &str,
    language: Language,
    in_rust_impl: bool,
) -> Option<(String, SymbolKind)> {
    match language {
        Language::Rust => {
            if let Some(name) = after_keyword(line, "fn ") {
                Some((
                    name,
                    if in_rust_impl {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    },
                ))
            } else if line.starts_with("struct ") || line.starts_with("pub struct ") {
                after_keyword(line, "struct ").map(|name| (name, SymbolKind::Class))
            } else if line.starts_with("enum ") || line.starts_with("pub enum ") {
                after_keyword(line, "enum ").map(|name| (name, SymbolKind::Class))
            } else {
                None
            }
        }
        Language::TypeScript | Language::JavaScript => {
            if let Some(name) = after_keyword(line, "function ") {
                Some((name, SymbolKind::Function))
            } else if let Some(name) = const_function_name(line) {
                Some((name, SymbolKind::Function))
            } else if let Some(name) = after_keyword(line, "class ") {
                Some((name, SymbolKind::Class))
            } else {
                None
            }
        }
        Language::Python => {
            if let Some(name) = after_keyword(line, "def ") {
                Some((name, SymbolKind::Function))
            } else {
                after_keyword(line, "class ").map(|name| (name, SymbolKind::Class))
            }
        }
        Language::Go => {
            if let Some(name) = after_keyword(line, "func ") {
                let name = if name.starts_with('(') {
                    line.split(')')
                        .nth(1)
                        .and_then(|rest| identifier_before(rest, '('))?
                } else {
                    name
                };
                Some((name, SymbolKind::Function))
            } else if let Some(rest) = line.strip_prefix("type ") {
                identifier_before(rest, ' ').map(|name| (name, SymbolKind::Class))
            } else {
                None
            }
        }
        Language::Unknown => None,
    }
}

fn parse_calls(line: &str, language: Language) -> Vec<String> {
    let mut calls = Vec::new();
    let mut token = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }
        if ch == '(' && !token.is_empty() && !is_call_keyword(&token, language) {
            calls.push(token.clone());
        }
        token.clear();
        if ch == '"' || ch == '\'' {
            for next in chars.by_ref() {
                if next == ch {
                    break;
                }
            }
        }
    }
    calls
}

fn parse_rust_impl_target(line: &str) -> Option<String> {
    let rest = line.strip_prefix("impl ")?.trim();
    let rest = rest
        .strip_prefix('<')
        .and_then(|value| value.split_once('>').map(|(_, after)| after.trim()))
        .unwrap_or(rest);
    let target = if let Some((_, target)) = rest.split_once(" for ") {
        target
    } else {
        rest
    };
    target
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .find(|part| !part.is_empty())
        .map(str::to_string)
}

fn after_keyword(line: &str, keyword: &str) -> Option<String> {
    let start = line.find(keyword)? + keyword.len();
    let rest = &line[start..];
    identifier_before(rest, '(')
        .or_else(|| identifier_before(rest, '<'))
        .or_else(|| identifier_before(rest, '{'))
        .or_else(|| identifier_before(rest, ':'))
        .or_else(|| identifier_before(rest, ' '))
}

fn identifier_before(rest: &str, delimiter: char) -> Option<String> {
    let candidate = rest.split(delimiter).next()?.trim();
    let ident = candidate
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    (!ident.is_empty()).then_some(ident)
}

fn const_function_name(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("let "))
        .or_else(|| line.strip_prefix("var "))?;
    let (name, rhs) = rest.split_once('=')?;
    if rhs.contains("=>") || rhs.trim_start().starts_with("function") {
        Some(name.trim().to_string())
    } else {
        None
    }
}

fn clean_js_module(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(';')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn is_comment(line: &str, language: Language) -> bool {
    match language {
        Language::Python => line.starts_with('#'),
        _ => line.starts_with("//") || line.starts_with("/*") || line.starts_with('*'),
    }
}

fn is_call_keyword(token: &str, language: Language) -> bool {
    matches!(
        token,
        "if" | "for"
            | "while"
            | "match"
            | "switch"
            | "return"
            | "sizeof"
            | "Some"
            | "Ok"
            | "Err"
            | "vec"
            | "println"
            | "format"
    ) || (language == Language::Python && matches!(token, "print" | "len" | "str" | "int"))
}

fn is_exported_symbol(line: &str, language: Language) -> bool {
    match language {
        Language::Rust => line.starts_with("pub "),
        Language::TypeScript | Language::JavaScript => line.starts_with("export "),
        Language::Go => parse_symbol(line, language, false)
            .is_some_and(|(name, _)| name.chars().next().is_some_and(char::is_uppercase)),
        Language::Python => {
            parse_symbol(line, language, false).is_some_and(|(name, _)| !name.starts_with('_'))
        }
        Language::Unknown => false,
    }
}

impl Language {
    fn from_path(path: &str) -> Self {
        match Path::new(path).extension().and_then(|ext| ext.to_str()) {
            Some("rs") => Self::Rust,
            Some("ts") | Some("tsx") => Self::TypeScript,
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Self::JavaScript,
            Some("py") => Self::Python,
            Some("go") => Self::Go,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_symbols_and_calls() {
        let file = parse_source_file(
            "src/lib.rs",
            r#"
use crate::other;
pub fn run() {
  helper();
}
fn helper() {}
"#,
        );

        assert_eq!(file.imports[0].module, "crate::other");
        assert_eq!(file.symbols[0].name, "run");
        assert_eq!(
            file.symbols[0].range,
            SymbolRange {
                start_line: 3,
                end_line: 5
            }
        );
        assert_eq!(file.symbols[0].calls[0].target, "helper");
    }

    #[test]
    fn marks_rust_impl_functions_as_methods() {
        let file = parse_source_file(
            "src/lib.rs",
            r#"
impl Thing {
  pub fn method(&self) {}
}
"#,
        );

        assert_eq!(file.symbols[0].kind, SymbolKind::Method);
        assert_eq!(file.symbols[0].parent.as_deref(), Some("Thing"));
    }

    #[test]
    fn extracts_python_symbols() {
        let file = parse_source_file(
            "x.py",
            r#"
import os
def main():
    work()
def work():
    pass
"#,
        );

        assert_eq!(file.imports[0].module, "os");
        assert_eq!(
            file.symbols[0].range,
            SymbolRange {
                start_line: 3,
                end_line: 4
            }
        );
        assert_eq!(file.symbols[0].calls[0].target, "work");
    }

    #[test]
    fn marks_python_class_functions_as_methods() {
        let file = parse_source_file(
            "x.py",
            r#"
class Worker:
    def run(self):
        helper()
"#,
        );

        assert_eq!(file.symbols[0].kind, SymbolKind::Class);
        assert_eq!(file.symbols[1].kind, SymbolKind::Method);
        assert_eq!(file.symbols[1].parent.as_deref(), Some("Worker"));
    }

    #[test]
    fn attack_surface_keeps_exported_symbols_even_when_called() {
        let file = parse_source_file(
            "src/lib.rs",
            r#"
pub fn api() {
  helper();
}
fn caller() {
  api();
}
fn helper() {}
"#,
        );
        let mut graph = SourceGraph::default();
        for symbol in &file.symbols {
            graph
                .symbol_index
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol.id.clone());
        }
        graph.files.insert(file.path.clone(), file);

        let rows = graph.attack_surface();
        assert!(rows.contains(&"src/lib.rs#api".to_string()));
        assert!(rows.contains(&"src/lib.rs#caller".to_string()));
        assert!(!rows.contains(&"src/lib.rs#helper".to_string()));
    }

    #[test]
    fn call_resolution_prefers_symbols_in_the_callers_file() {
        let local = parse_source_file(
            "src/local.rs",
            r#"
fn caller() {
  target();
}
fn target() {}
"#,
        );
        let other = parse_source_file("src/other.rs", "fn target() {}\n");
        let mut graph = SourceGraph::default();
        for file in [local, other] {
            for symbol in &file.symbols {
                graph
                    .symbol_index
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(symbol.id.clone());
            }
            graph.files.insert(file.path.clone(), file);
        }

        assert_eq!(
            graph.callees("src/local.rs#caller"),
            vec!["src/local.rs#target"]
        );
    }

    #[test]
    fn extracts_typescript_symbols_imports_and_calls() {
        let file = parse_source_file(
            "src/app.ts",
            r#"
import { api } from "./api";
export class Service {
  run() {
    api();
  }
}
const helper = () => api();
"#,
        );

        assert_eq!(file.imports[0].module, "./api");
        assert!(file.symbols.iter().any(|symbol| symbol.name == "Service"
            && symbol.kind == SymbolKind::Class
            && symbol.exported));
        assert!(file.symbols.iter().any(|symbol| symbol.name == "run"
            && symbol.kind == SymbolKind::Method
            && symbol.parent.as_deref() == Some("Service")));
        assert!(file.symbols.iter().any(|symbol| symbol.name == "helper"));
    }

    #[test]
    fn extracts_go_symbols_imports_and_calls() {
        let file = parse_source_file(
            "main.go",
            r#"
package main
import "fmt"
type Server struct {}
func main() {
  fmt.Println("hi")
}
"#,
        );

        assert_eq!(file.imports[0].module, "fmt");
        assert!(file.symbols.iter().any(|symbol| symbol.name == "Server"
            && symbol.kind == SymbolKind::Class
            && symbol.exported));
        let main = file
            .symbols
            .iter()
            .find(|symbol| symbol.name == "main")
            .unwrap();
        assert_eq!(main.calls[0].target, "Println");
    }
}

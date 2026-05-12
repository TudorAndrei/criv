use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Result;
use crate::util::read_to_string;

#[derive(Debug, Default, Clone)]
pub(crate) struct SourceGraph {
    pub(crate) files: BTreeMap<String, SourceFile>,
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
    pub(crate) line: usize,
    pub(crate) calls: Vec<Call>,
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
    pub(crate) fn build(root: &Path, source_files: &[String]) -> Result<Self> {
        let mut graph = Self::default();
        for source_file in source_files {
            let path = root.join(source_file);
            let contents = read_to_string(&path)?;
            let parsed = parse_source_file(source_file, &contents);
            for symbol in &parsed.symbols {
                graph
                    .symbol_index
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(symbol.id.clone());
            }
            graph.files.insert(source_file.clone(), parsed);
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
                        self.resolve_symbol(&call.target)
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
                    self.resolve_symbol(&call.target)
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
                    if let Some(target) = self.resolve_symbol(&call.target) {
                        called.insert(target);
                    }
                }
            }
        }

        let mut rows = self
            .files
            .values()
            .flat_map(|file| &file.symbols)
            .filter(|symbol| !called.contains(&symbol.id))
            .map(|symbol| symbol.id.display())
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }

    pub(crate) fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.files.values().flat_map(|file| file.symbols.iter())
    }

    fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.files
            .get(&id.path)
            .and_then(|file| file.symbols.iter().find(|symbol| &symbol.id == id))
    }
}

fn parse_source_file(path: &str, contents: &str) -> SourceFile {
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
    let mut python_indent: Option<usize> = None;

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
            if let Some(active_indent) = python_indent {
                if indent <= active_indent && !trimmed.starts_with('@') {
                    current_symbol = None;
                    python_indent = None;
                }
            }
        }

        let in_rust_impl = language == Language::Rust && rust_impl_depth > 0;
        if let Some((name, kind)) = parse_symbol(trimmed, language, in_rust_impl) {
            let id = SymbolId {
                path: path.into(),
                name: name.clone(),
            };
            file.symbols.push(Symbol {
                id,
                name,
                kind,
                line: line_no,
                calls: Vec::new(),
            });
            current_symbol = Some(file.symbols.len() - 1);
            if language == Language::Python {
                python_indent = Some(line.len() - line.trim_start().len());
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
                rust_impl_depth += line.matches('{').count().max(1) as isize;
            } else if rust_impl_depth > 0 {
                rust_impl_depth += line.matches('{').count() as isize;
                rust_impl_depth -= line.matches('}').count() as isize;
                rust_impl_depth = rust_impl_depth.max(0);
            }
            brace_depth += line.matches('{').count() as isize;
            brace_depth -= line.matches('}').count() as isize;
            if brace_depth <= 0 {
                current_symbol = None;
                brace_depth = 0;
            }
        }
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
        assert_eq!(file.symbols[0].calls[0].target, "work");
    }
}

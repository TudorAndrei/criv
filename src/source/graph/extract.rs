//! Per-language Source extraction: one table row per language and the
//! generic Tree-sitter extractor that reads the rows.

use std::path::Path;

use tree_sitter::{Node, Parser};

use super::syntax::{
    Depth, descendants, field_text, first_named_child, matching_close, node_range, node_text,
    split_top_level,
};
use super::{
    Call, FieldSignature, Import, InterfaceSignature, Language, ModuleDecl, SourceFile, Symbol,
    SymbolId, SymbolKind, VariantSignature, elixir,
};
use crate::{CrivError, Result};

/// Everything criv knows about one language.
struct LanguageSpec {
    language: Language,
    name: &'static str,
    extensions: &'static [&'static str],
    /// Overrides the extension-based media type of the file.
    mime: Option<&'static str>,
    grammar: Option<Grammar>,
    /// The symbol name that, with every uncalled top-level symbol, counts as
    /// an entry point. Without it only exported symbols do.
    entry_point: Option<&'static str>,
}

struct Grammar {
    load: fn() -> tree_sitter::Language,
    rules: Rules,
}

enum Rules {
    Generic(GenericRules),
    /// Elixir meaning lives in the Elixir module (ADR-0129).
    Elixir,
}

/// The node kinds and text forms the generic extractor reads.
struct GenericRules {
    symbols: &'static [(&'static str, SymbolShape)],
    modules: &'static [(&'static str, NameSource)],
    imports: &'static [(&'static str, ImportForm)],
    /// The node kinds of a call.
    calls: &'static [&'static str],
    container: Option<Container>,
    /// Call targets that are keywords or built-ins, beyond the common ones.
    call_keywords: &'static [&'static str],
    exported: ExportRule,
    visibility: VisibilityRule,
    signature: SignatureStyle,
}

/// A node whose text names the parent of the symbols inside it.
#[derive(Clone, Copy)]
struct Container {
    node: &'static str,
    parent: fn(&str) -> Option<String>,
}

#[derive(Clone, Copy)]
enum SymbolShape {
    /// A method inside a parent, a function otherwise.
    Callable,
    Function,
    Method,
    Class,
    /// A variable whose value contains a node of one of these kinds.
    FunctionValue(&'static [&'static str]),
}

#[derive(Clone, Copy)]
enum NameSource {
    Field(&'static str),
    Descendant(&'static str),
}

#[derive(Clone, Copy)]
enum ImportForm {
    RustUse,
    PythonStatement,
    /// The first quoted literal of these kinds names the module.
    Quoted(&'static [&'static str]),
}

#[derive(Clone, Copy)]
enum ExportRule {
    VisibilityModifier,
    ExportStatement,
    UppercaseName,
    NoLeadingUnderscore,
}

#[derive(Clone, Copy)]
enum VisibilityRule {
    RustPub,
    Fixed(&'static str),
}

#[derive(Clone, Copy)]
enum SignatureStyle {
    Rust,
    TypeScript,
    Plain,
}

const COMMON_CALL_KEYWORDS: &[&str] = &[
    "if", "for", "while", "match", "switch", "return", "sizeof", "Some", "Ok", "Err", "vec",
    "println", "format",
];

const TYPESCRIPT_RULES: GenericRules = GenericRules {
    symbols: &[
        ("class_declaration", SymbolShape::Class),
        ("function_declaration", SymbolShape::Function),
        ("generator_function_declaration", SymbolShape::Function),
        ("method_definition", SymbolShape::Method),
        ("method_signature", SymbolShape::Method),
        (
            "variable_declarator",
            SymbolShape::FunctionValue(&["arrow_function", "function_expression"]),
        ),
    ],
    modules: &[("internal_module", NameSource::Field("name"))],
    imports: &[("import_statement", ImportForm::Quoted(&["string"]))],
    calls: &["call_expression"],
    container: None,
    call_keywords: &[],
    exported: ExportRule::ExportStatement,
    visibility: VisibilityRule::Fixed("export"),
    signature: SignatureStyle::TypeScript,
};

static UNKNOWN: LanguageSpec = LanguageSpec {
    language: Language::Unknown,
    name: "unknown",
    extensions: &[],
    mime: None,
    grammar: None,
    entry_point: None,
};

static LANGUAGES: [LanguageSpec; 6] = [
    LanguageSpec {
        language: Language::Rust,
        name: "rust",
        extensions: &["rs"],
        mime: None,
        grammar: Some(Grammar {
            load: || tree_sitter_rust::LANGUAGE.into(),
            rules: Rules::Generic(GenericRules {
                symbols: &[
                    ("function_item", SymbolShape::Callable),
                    ("struct_item", SymbolShape::Class),
                    ("enum_item", SymbolShape::Class),
                ],
                modules: &[("mod_item", NameSource::Field("name"))],
                imports: &[("use_declaration", ImportForm::RustUse)],
                calls: &["call_expression"],
                container: Some(Container {
                    node: "impl_item",
                    parent: rust_impl_target,
                }),
                call_keywords: &[],
                exported: ExportRule::VisibilityModifier,
                visibility: VisibilityRule::RustPub,
                signature: SignatureStyle::Rust,
            }),
        }),
        entry_point: Some("main"),
    },
    LanguageSpec {
        language: Language::TypeScript,
        name: "typescript",
        extensions: &["ts", "tsx"],
        mime: None,
        grammar: Some(Grammar {
            load: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            rules: Rules::Generic(TYPESCRIPT_RULES),
        }),
        entry_point: Some("main"),
    },
    LanguageSpec {
        language: Language::JavaScript,
        name: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        mime: None,
        grammar: Some(Grammar {
            load: || tree_sitter_javascript::LANGUAGE.into(),
            rules: Rules::Generic(TYPESCRIPT_RULES),
        }),
        entry_point: Some("main"),
    },
    LanguageSpec {
        language: Language::Python,
        name: "python",
        extensions: &["py"],
        mime: None,
        grammar: Some(Grammar {
            load: || tree_sitter_python::LANGUAGE.into(),
            rules: Rules::Generic(GenericRules {
                symbols: &[
                    ("function_definition", SymbolShape::Callable),
                    ("class_definition", SymbolShape::Class),
                ],
                modules: &[],
                imports: &[
                    ("import_statement", ImportForm::PythonStatement),
                    ("import_from_statement", ImportForm::PythonStatement),
                ],
                calls: &["call"],
                container: None,
                call_keywords: &["print", "len", "str", "int"],
                exported: ExportRule::NoLeadingUnderscore,
                visibility: VisibilityRule::Fixed("public"),
                signature: SignatureStyle::Plain,
            }),
        }),
        entry_point: Some("main"),
    },
    LanguageSpec {
        language: Language::Go,
        name: "go",
        extensions: &["go"],
        mime: None,
        grammar: Some(Grammar {
            load: || tree_sitter_go::LANGUAGE.into(),
            rules: Rules::Generic(GenericRules {
                symbols: &[
                    ("type_spec", SymbolShape::Class),
                    ("method_declaration", SymbolShape::Method),
                    ("function_declaration", SymbolShape::Function),
                ],
                modules: &[(
                    "package_clause",
                    NameSource::Descendant("package_identifier"),
                )],
                imports: &[(
                    "import_spec",
                    ImportForm::Quoted(&["interpreted_string_literal", "raw_string_literal"]),
                )],
                calls: &["call_expression"],
                container: None,
                call_keywords: &[],
                exported: ExportRule::UppercaseName,
                visibility: VisibilityRule::Fixed("public"),
                signature: SignatureStyle::Plain,
            }),
        }),
        entry_point: Some("main"),
    },
    LanguageSpec {
        language: Language::Elixir,
        name: "elixir",
        extensions: &["ex", "exs"],
        mime: Some("text/x-elixir"),
        grammar: Some(Grammar {
            load: || tree_sitter_elixir::LANGUAGE.into(),
            rules: Rules::Elixir,
        }),
        entry_point: None,
    },
];

impl Language {
    pub(crate) fn from_path(path: impl AsRef<Path>) -> Self {
        let Some(extension) = path
            .as_ref()
            .extension()
            .and_then(|extension| extension.to_str())
        else {
            return Self::Unknown;
        };
        LANGUAGES
            .iter()
            .find(|spec| spec.extensions.contains(&extension))
            .map_or(Self::Unknown, |spec| spec.language)
    }

    fn spec(self) -> &'static LanguageSpec {
        LANGUAGES
            .iter()
            .find(|spec| spec.language == self)
            .unwrap_or(&UNKNOWN)
    }

    pub(crate) fn as_str(self) -> &'static str {
        self.spec().name
    }

    pub(crate) fn mime(self) -> Option<&'static str> {
        self.spec().mime
    }

    pub(super) fn entry_point(self) -> Option<&'static str> {
        self.spec().entry_point
    }
}

/// Parses one file. Tree-sitter recovers from invalid syntax, so a file
/// with errors keeps the declarations of its valid parts.
///
/// # Errors
///
/// Returns an error when the grammar of the language cannot be loaded.
pub(super) fn parse_source_file(path: &str, contents: &str) -> Result<SourceFile> {
    let language = Language::from_path(path);
    let Some(grammar) = &language.spec().grammar else {
        return Ok(SourceFile::new(path, language));
    };
    let mut parser = Parser::new();
    parser.set_language(&(grammar.load)()).map_err(|error| {
        CrivError::new(format!(
            "failed to load the {} grammar: {error}",
            language.as_str()
        ))
    })?;
    let Some(tree) = parser.parse(contents, None) else {
        return Ok(SourceFile::new(path, language));
    };
    Ok(match &grammar.rules {
        Rules::Elixir => elixir::parse_file(path, contents, tree.root_node()),
        Rules::Generic(rules) => {
            let mut file = SourceFile::new(path, language);
            GenericWalk {
                contents,
                path,
                language,
                rules,
            }
            .collect(tree.root_node(), None, None, &mut file, Depth::ROOT);
            file.resolve_container_parents();
            file
        }
    })
}

/// The parent a walk passes to the symbols below it.
#[derive(Clone)]
struct Parent {
    name: String,
    /// The parent symbol, when the parent is a symbol the walk has seen.
    id: Option<SymbolId>,
}

impl SourceFile {
    /// Points each symbol whose parent came from a container, such as a Rust
    /// `impl`, at the one type symbol of that name in the file.
    fn resolve_container_parents(&mut self) {
        let types = self
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Class)
            .map(|symbol| (symbol.name.clone(), symbol.id.clone()))
            .collect::<Vec<_>>();
        for symbol in &mut self.symbols {
            let Some(parent) = symbol
                .parent
                .as_deref()
                .filter(|_| symbol.parent_id.is_none())
            else {
                continue;
            };
            let mut matches = types.iter().filter(|(name, _)| name == parent);
            if let (Some((_, id)), None) = (matches.next(), matches.next()) {
                symbol.parent_id = Some(id.clone());
            }
        }
    }
}

struct GenericWalk<'a> {
    contents: &'a str,
    path: &'a str,
    language: Language,
    rules: &'a GenericRules,
}

impl GenericWalk<'_> {
    fn collect(
        &self,
        node: Node<'_>,
        parent: Option<Parent>,
        module_parent: Option<String>,
        file: &mut SourceFile,
        depth: Depth,
    ) {
        let line = node.start_position().row.saturating_add(1);
        for import in self.imports(node) {
            file.imports
                .push(Import::unscoped(import, line, node.start_byte()));
        }
        let child_module_parent = if let Some(name) = self.module(node) {
            let name = module_parent
                .as_deref()
                .map_or_else(|| name.clone(), |parent| format!("{parent}::{name}"));
            file.modules.push(ModuleDecl {
                name: name.clone(),
                line,
            });
            Some(name)
        } else {
            module_parent
        };

        let child_parent = if let Some(symbol) = self.symbol(node, parent.as_ref()) {
            let symbol_parent = if symbol.kind == SymbolKind::Class {
                Some(Parent {
                    name: symbol.name.clone(),
                    id: Some(symbol.id.clone()),
                })
            } else {
                parent
            };
            file.symbols.push(symbol);
            symbol_parent
        } else if let Some(container) = self.rules.container
            && node.kind() == container.node
        {
            node_text(node, self.contents)
                .and_then(|text| (container.parent)(&text))
                .map(|name| Parent { name, id: None })
        } else {
            parent
        };

        let Some(depth) = depth.child() else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(
                child,
                child_parent.clone(),
                child_module_parent.clone(),
                file,
                depth,
            );
        }
    }

    fn module(&self, node: Node<'_>) -> Option<String> {
        let (_, source) = self
            .rules
            .modules
            .iter()
            .find(|(kind, _)| *kind == node.kind())?;
        match source {
            NameSource::Field(field) => field_text(node, self.contents, field),
            NameSource::Descendant(kind) => descendants(node)
                .find(|child| child.kind() == *kind)
                .and_then(|name| node_text(name, self.contents)),
        }
    }

    fn imports(&self, node: Node<'_>) -> Vec<String> {
        let Some((_, form)) = self
            .rules
            .imports
            .iter()
            .find(|(kind, _)| *kind == node.kind())
        else {
            return Vec::new();
        };
        match form {
            ImportForm::RustUse => node_text(node, self.contents)
                .map(|text| rust_use_imports(&text))
                .unwrap_or_default(),
            ImportForm::PythonStatement => node_text(node, self.contents)
                .and_then(|text| python_import(text.trim()))
                .into_iter()
                .collect(),
            ImportForm::Quoted(kinds) => kinds
                .iter()
                .find_map(|kind| descendants(node).find(|child| child.kind() == *kind))
                .and_then(|child| node_text(child, self.contents))
                .map(|text| unquote_module(&text))
                .into_iter()
                .collect(),
        }
    }

    fn symbol(&self, node: Node<'_>, parent: Option<&Parent>) -> Option<Symbol> {
        let (_, shape) = self
            .rules
            .symbols
            .iter()
            .find(|(kind, _)| *kind == node.kind())?;
        let kind = match shape {
            SymbolShape::Callable if parent.is_some() => SymbolKind::Method,
            SymbolShape::Callable | SymbolShape::Function => SymbolKind::Function,
            SymbolShape::FunctionValue(values) => {
                if !descendants(node).any(|child| values.contains(&child.kind())) {
                    return None;
                }
                SymbolKind::Function
            }
            SymbolShape::Method => SymbolKind::Method,
            SymbolShape::Class => SymbolKind::Class,
        };
        let name = field_text(node, self.contents, "name")?;
        let source = node_text(node, self.contents).unwrap_or_default();
        let exported = self.exported(node);
        let parent_name = parent.map(|parent| parent.name.as_str());
        let interface_signature =
            Some(self.interface_signature(kind, &name, parent_name, exported, &source));
        let selector = symbol_selector(kind, parent_name, &name);
        Some(Symbol {
            parent: parent_name.map(str::to_string),
            parent_id: parent.and_then(|parent| parent.id.clone()),
            exported,
            interface_signature,
            calls: self.calls(node),
            ..Symbol::new(self.path, name, kind, selector, node_range(node))
        })
    }

    fn exported(&self, node: Node<'_>) -> bool {
        match self.rules.exported {
            ExportRule::VisibilityModifier => {
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .any(|child| child.kind() == "visibility_modifier")
            }
            ExportRule::ExportStatement => {
                node.parent()
                    .is_some_and(|parent| parent.kind() == "export_statement")
                    || node_text(node, self.contents)
                        .is_some_and(|text| text.trim_start().starts_with("export "))
            }
            ExportRule::UppercaseName => field_text(node, self.contents, "name")
                .is_some_and(|name| name.chars().next().is_some_and(char::is_uppercase)),
            ExportRule::NoLeadingUnderscore => {
                field_text(node, self.contents, "name").is_some_and(|name| !name.starts_with('_'))
            }
        }
    }

    fn calls(&self, node: Node<'_>) -> Vec<Call> {
        let mut calls = descendants(node)
            .filter(|node| self.rules.calls.contains(&node.kind()))
            .filter_map(|node| {
                let function = node
                    .child_by_field_name("function")
                    .or_else(|| first_named_child(node))?;
                let target = call_target(function, self.contents)?;
                (!self.is_call_keyword(&target)).then(|| Call {
                    target,
                    line: node.start_position().row.saturating_add(1),
                })
            })
            .collect::<Vec<_>>();
        calls.sort_by(|left, right| (left.line, &left.target).cmp(&(right.line, &right.target)));
        calls.dedup_by(|left, right| left.line == right.line && left.target == right.target);
        calls
    }

    fn is_call_keyword(&self, target: &str) -> bool {
        COMMON_CALL_KEYWORDS.contains(&target) || self.rules.call_keywords.contains(&target)
    }

    fn interface_signature(
        &self,
        symbol_kind: SymbolKind,
        name: &str,
        parent: Option<&str>,
        exported: bool,
        source: &str,
    ) -> InterfaceSignature {
        let qualified_name =
            parent.map_or_else(|| name.to_string(), |parent| format!("{parent}.{name}"));
        let visibility = exported.then(|| match self.rules.visibility {
            VisibilityRule::RustPub => rust_visibility(source),
            VisibilityRule::Fixed(text) => text.to_string(),
        });
        let style = self.rules.signature;
        let (inputs, output) = match symbol_kind {
            SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Macro
            | SymbolKind::Guard
            | SymbolKind::Callback
            | SymbolKind::MacroCallback => function_signature(style, source),
            SymbolKind::Class
            | SymbolKind::Module
            | SymbolKind::Protocol
            | SymbolKind::Implementation
            | SymbolKind::Struct
            | SymbolKind::Exception
            | SymbolKind::Behaviour => (Vec::new(), None),
        };
        let (fields, variants) = match symbol_kind {
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::Exception => {
                aggregate_signature(style, source)
            }
            SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Module
            | SymbolKind::Protocol
            | SymbolKind::Implementation
            | SymbolKind::Behaviour
            | SymbolKind::Macro
            | SymbolKind::Guard
            | SymbolKind::Callback
            | SymbolKind::MacroCallback => (Vec::new(), Vec::new()),
        };

        InterfaceSignature {
            language: self.language,
            symbol_kind,
            qualified_name,
            visibility,
            inputs,
            output,
            fields,
            variants,
            arity: None,
            guards: Vec::new(),
            defaults: Vec::new(),
            specifications: Vec::new(),
        }
    }
}

fn symbol_selector(kind: SymbolKind, parent: Option<&str>, name: &str) -> String {
    match (kind, parent) {
        (SymbolKind::Function, _) => format!("fn:{name}"),
        (SymbolKind::Method, Some(parent)) => format!("type:{parent}/member:{name}"),
        (SymbolKind::Method, None) => format!("member:{name}"),
        (SymbolKind::Class, _) => format!("type:{name}"),
        _ => format!("{}:{name}", kind.as_str()),
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

fn unquote_module(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(';')
        .trim_matches(['"', '\'', '`'])
        .to_string()
}

fn python_import(statement: &str) -> Option<String> {
    statement
        .strip_prefix("import ")
        .map(|value| value.split_whitespace().next().unwrap_or(value).to_string())
        .or_else(|| {
            statement
                .strip_prefix("from ")
                .and_then(|value| value.split_once(" import "))
                .map(|(module, _)| module.to_string())
        })
}

fn rust_use_imports(declaration: &str) -> Vec<String> {
    let declaration = declaration.trim();
    let Some(body) = declaration
        .strip_prefix("use ")
        .or_else(|| declaration.strip_prefix("pub use "))
    else {
        return Vec::new();
    };
    let body = body.trim().trim_end_matches(';').trim();
    let mut rows = expand_rust_import("", body);
    rows.sort();
    rows.dedup();
    rows
}

fn expand_rust_import(prefix: &str, body: &str) -> Vec<String> {
    let body = body.trim();
    if body.is_empty() {
        return Vec::new();
    }

    if let Some(open) = body.find('{')
        && let Some(close) = matching_close(body, open, '{', '}')
    {
        let Some(head) = body.get(..open).map(str::trim) else {
            return Vec::new();
        };
        let base = join_rust_import(prefix, head.trim_end_matches("::"));
        let Some(children) = open
            .checked_add(1)
            .and_then(|body_start| body.get(body_start..close))
        else {
            return Vec::new();
        };
        return split_top_level(children, ',')
            .into_iter()
            .flat_map(|part| expand_rust_import(&base, part))
            .collect();
    }

    let leaf = body
        .split_once(" as ")
        .map_or(body, |(module, _)| module)
        .trim();
    let module = join_rust_import(prefix, leaf);
    if module.is_empty() {
        Vec::new()
    } else {
        vec![module]
    }
}

fn join_rust_import(prefix: &str, leaf: &str) -> String {
    let leaf = leaf.trim().trim_matches(':');
    if leaf.is_empty() || leaf == "self" {
        return prefix.to_string();
    }
    let leaf = leaf.strip_prefix("self::").unwrap_or(leaf);
    if prefix.is_empty() {
        leaf.to_string()
    } else {
        format!("{}::{}", prefix.trim_end_matches("::"), leaf)
    }
}

fn rust_impl_target(text: &str) -> Option<String> {
    let rest = text.strip_prefix("impl ")?.trim();
    let rest = rest
        .strip_prefix('<')
        .and_then(|value| value.split_once('>').map(|(_, after)| after.trim()))
        .unwrap_or(rest);
    let target = rest.split_once(" for ").map_or(rest, |(_, target)| target);
    target
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .find(|part| !part.is_empty())
        .map(str::to_string)
}

fn rust_visibility(source: &str) -> String {
    let trimmed = source.trim_start();
    if trimmed.starts_with("pub(") {
        trimmed
            .split_whitespace()
            .next()
            .unwrap_or("pub")
            .trim_end_matches('{')
            .to_string()
    } else {
        "pub".into()
    }
}

fn function_signature(style: SignatureStyle, source: &str) -> (Vec<String>, Option<String>) {
    let (header, output_marker) = match style {
        SignatureStyle::Rust => (
            source.split_once('{').map_or(source, |(head, _)| head),
            "->",
        ),
        SignatureStyle::TypeScript => (
            source
                .split_once("=>")
                .map(|(head, _)| head)
                .or_else(|| source.split_once('{').map(|(head, _)| head))
                .unwrap_or(source),
            ":",
        ),
        SignatureStyle::Plain => {
            let header = source.split_once('{').map_or(source, |(head, _)| head);
            return (signature_inputs(header.trim()), None);
        }
    };
    let header = header.trim();
    let output = header
        .rsplit_once(')')
        .map(|(_, tail)| tail.trim())
        .and_then(|tail| tail.strip_prefix(output_marker))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_signature_text);
    (signature_inputs(header), output)
}

fn signature_inputs(header: &str) -> Vec<String> {
    paren_contents(header)
        .map(|inputs| {
            inputs
                .split(',')
                .map(normalize_signature_text)
                .filter(|value| !value.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn aggregate_signature(
    style: SignatureStyle,
    source: &str,
) -> (Vec<FieldSignature>, Vec<VariantSignature>) {
    match style {
        SignatureStyle::Rust
            if source.trim_start().starts_with("enum ") || source.contains(" enum ") =>
        {
            (Vec::new(), rust_enum_variants(source))
        }
        SignatureStyle::Rust => (rust_fields(source), Vec::new()),
        SignatureStyle::TypeScript => (typescript_members(source), Vec::new()),
        SignatureStyle::Plain => (Vec::new(), Vec::new()),
    }
}

fn rust_fields(source: &str) -> Vec<FieldSignature> {
    brace_contents(source)
        .map(|body| {
            body.lines()
                .filter_map(|line| {
                    let line = line
                        .trim()
                        .trim_start_matches("pub ")
                        .trim_end_matches(',')
                        .trim();
                    let (name, ty) = line.split_once(':')?;
                    Some(FieldSignature {
                        name: name.trim().to_string(),
                        ty: Some(normalize_signature_text(ty)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn rust_enum_variants(source: &str) -> Vec<VariantSignature> {
    brace_contents(source)
        .map(|body| {
            body.lines()
                .filter_map(|line| {
                    let line = line.trim().trim_end_matches(',').trim();
                    if line.is_empty() || line.starts_with("//") {
                        return None;
                    }
                    let name = line
                        .split(['(', '{', '='])
                        .next()
                        .unwrap_or(line)
                        .trim()
                        .to_string();
                    (!name.is_empty()).then_some(VariantSignature {
                        name,
                        fields: Vec::new(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn typescript_members(source: &str) -> Vec<FieldSignature> {
    brace_contents(source)
        .map(|body| {
            body.lines()
                .filter_map(|line| {
                    let line = line
                        .trim()
                        .trim_start_matches("public ")
                        .trim_start_matches("readonly ")
                        .trim_end_matches(';')
                        .trim_end_matches(',')
                        .trim();
                    if line.is_empty() || line.starts_with("//") || line.starts_with("constructor")
                    {
                        return None;
                    }
                    if let Some((name, _)) = line.split_once('(') {
                        let output = line
                            .rsplit_once(')')
                            .and_then(|(_, tail)| tail.trim().strip_prefix(':'))
                            .map(normalize_signature_text);
                        return Some(FieldSignature {
                            name: name.trim().to_string(),
                            ty: output,
                        });
                    }
                    let (name, ty) = line.split_once(':')?;
                    Some(FieldSignature {
                        name: name.trim().trim_end_matches('?').to_string(),
                        ty: Some(normalize_signature_text(ty)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn paren_contents(source: &str) -> Option<&str> {
    let start = source.find('(')?;
    let end = source.rfind(')')?;
    (end > start)
        .then(|| start.checked_add(1))
        .flatten()
        .and_then(|body_start| source.get(body_start..end))
}

fn brace_contents(source: &str) -> Option<&str> {
    let start = source.find('{')?;
    let end = source.rfind('}')?;
    (end > start)
        .then(|| start.checked_add(1))
        .flatten()
        .and_then(|body_start| source.get(body_start..end))
}

fn normalize_signature_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

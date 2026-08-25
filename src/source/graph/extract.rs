use tree_sitter::{Node, Parser};

use super::{
    Call, Import, InterfaceSignature, Language, ModuleDecl, SourceFile, Symbol, SymbolId,
    SymbolKind, SymbolRange, aggregate_signature, clean_js_module, descendant_of_kind, elixir,
    fallback_module_decl, field_text, first_named_child, function_signature, has_descendant_kind,
    is_call_keyword, is_comment, is_exported_symbol, node_text, parse_calls, parse_import,
    parse_imports, parse_rust_impl_target, parse_rust_imports, parse_symbol, symbol_selector,
    visibility_text,
};

const MAX_AST_DEPTH: usize = 512;

pub(super) fn parse_source_file(path: &str, contents: &str) -> SourceFile {
    parse_tree_sitter_file(path, contents).unwrap_or_else(|| {
        if Language::from_path(path) == Language::Elixir {
            SourceFile {
                path: path.into(),
                language: Language::Elixir,
                imports: Vec::new(),
                modules: Vec::new(),
                symbols: Vec::new(),
            }
        } else {
            parse_source_file_fallback(path, contents)
        }
    })
}

fn parse_source_file_fallback(path: &str, contents: &str) -> SourceFile {
    let language = Language::from_path(path);
    let mut file = SourceFile {
        path: path.into(),
        language,
        imports: Vec::new(),
        modules: Vec::new(),
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
        for import in parse_imports(trimmed, language) {
            file.imports.push(Import::legacy(import, line_no));
        }
        if let Some(name) = fallback_module_decl(trimmed, language) {
            file.modules.push(ModuleDecl {
                name,
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
            if let Some(active_indent) = python_indent
                && indent <= active_indent
                && !trimmed.starts_with('@')
            {
                if let Some(symbol_index) = current_symbol {
                    file.symbols[symbol_index].range.end_line = line_no.saturating_sub(1);
                }
                current_symbol = None;
                python_indent = None;
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
            let exported = is_exported_symbol(trimmed, language);
            let range = SymbolRange {
                start_line: line_no,
                end_line: total_lines,
            };
            file.symbols.push(Symbol {
                id: SymbolId {
                    path: path.into(),
                    name: name.clone(),
                    selector: symbol_selector(kind, parent.as_deref(), &name),
                },
                interface_signature: Some(interface_signature_from_source(
                    language,
                    kind,
                    &name,
                    parent.as_deref(),
                    exported,
                    trimmed,
                )),
                name,
                kind,
                parent,
                owner: None,
                arity: None,
                exported,
                range,
                clause_ranges: vec![range],
                calls: Vec::new(),
                relationships: Vec::new(),
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

fn parse_tree_sitter_file(path: &str, contents: &str) -> Option<SourceFile> {
    let language = Language::from_path(path);
    let tree_sitter_language = tree_sitter_language(language)?;
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_language).ok()?;
    let tree = parser.parse(contents, None)?;
    if language == Language::Elixir {
        return Some(elixir::parse_file(path, contents, tree.root_node()));
    }
    if tree.root_node().has_error() {
        return None;
    }

    let mut file = SourceFile {
        path: path.into(),
        language,
        imports: Vec::new(),
        modules: Vec::new(),
        symbols: Vec::new(),
    };
    TreeSitterWalk {
        contents,
        path,
        language,
    }
    .collect(tree.root_node(), None, None, &mut file, 0);
    Some(file)
}

fn tree_sitter_language(language: Language) -> Option<tree_sitter::Language> {
    match language {
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Language::Elixir => Some(tree_sitter_elixir::LANGUAGE.into()),
        Language::Unknown => None,
    }
}

struct TreeSitterWalk<'a> {
    contents: &'a str,
    path: &'a str,
    language: Language,
}

impl TreeSitterWalk<'_> {
    fn collect(
        &self,
        node: Node<'_>,
        parent: Option<String>,
        module_parent: Option<String>,
        file: &mut SourceFile,
        depth: usize,
    ) {
        if depth >= MAX_AST_DEPTH {
            return;
        }
        for import in tree_sitter_imports(node, self.contents, self.language) {
            file.imports
                .push(Import::legacy(import, node.start_position().row + 1));
        }
        let child_module_parent =
            if let Some(name) = tree_sitter_module(node, self.contents, self.language) {
                let name = module_parent
                    .as_deref()
                    .map_or_else(|| name.clone(), |parent| format!("{parent}::{name}"));
                file.modules.push(ModuleDecl {
                    name: name.clone(),
                    line: node.start_position().row + 1,
                });
                Some(name)
            } else {
                module_parent
            };

        if let Some(symbol) = tree_sitter_symbol(
            node,
            self.contents,
            self.path,
            self.language,
            parent.as_deref(),
        ) {
            let symbol_parent = if symbol.kind == SymbolKind::Class {
                Some(symbol.name.clone())
            } else {
                parent
            };
            file.symbols.push(symbol);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.collect(
                    child,
                    symbol_parent.clone(),
                    child_module_parent.clone(),
                    file,
                    depth + 1,
                );
            }
            return;
        }

        let impl_parent = if self.language == Language::Rust && node.kind() == "impl_item" {
            node_text(node, self.contents).and_then(|text| parse_rust_impl_target(&text))
        } else {
            parent
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(
                child,
                impl_parent.clone(),
                child_module_parent.clone(),
                file,
                depth + 1,
            );
        }
    }
}

fn tree_sitter_module(node: Node<'_>, contents: &str, language: Language) -> Option<String> {
    match (language, node.kind()) {
        (Language::Rust, "mod_item") => field_text(node, contents, "name"),
        (Language::TypeScript | Language::JavaScript, "internal_module") => {
            field_text(node, contents, "name")
        }
        (Language::Go, "package_clause") => descendant_of_kind(node, "package_identifier")
            .and_then(|name| node_text(name, contents)),
        _ => None,
    }
}

fn tree_sitter_imports(node: Node<'_>, contents: &str, language: Language) -> Vec<String> {
    match (language, node.kind()) {
        (Language::Rust, "use_declaration") => node_text(node, contents)
            .map(|text| parse_rust_imports(&text))
            .unwrap_or_default(),
        (Language::TypeScript | Language::JavaScript, "import_statement") => {
            descendant_of_kind(node, "string")
                .and_then(|child| node_text(child, contents))
                .map(|text: String| clean_js_module(&text))
                .into_iter()
                .collect()
        }
        (Language::Python, "import_statement" | "import_from_statement") => {
            node_text(node, contents)
                .and_then(|text| parse_import(text.trim(), language))
                .into_iter()
                .collect()
        }
        (Language::Go, "import_spec") => descendant_of_kind(node, "interpreted_string_literal")
            .or_else(|| descendant_of_kind(node, "raw_string_literal"))
            .and_then(|child| node_text(child, contents))
            .map(|text| text.trim_matches('"').trim_matches('`').to_string())
            .into_iter()
            .collect(),
        _ => Vec::new(),
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
        (
            Language::TypeScript | Language::JavaScript,
            "function_declaration" | "generator_function_declaration",
        ) => (field_text(node, contents, "name")?, SymbolKind::Function),
        (Language::TypeScript | Language::JavaScript, "method_definition" | "method_signature") => {
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

    let source = node_text(node, contents).unwrap_or_default();
    let exported = tree_sitter_exported(node, contents, language);
    let interface_signature = Some(interface_signature_from_source(
        language, kind, &name, parent, exported, &source,
    ));
    let range = SymbolRange {
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    };
    Some(Symbol {
        id: SymbolId {
            path: path.into(),
            name: name.clone(),
            selector: symbol_selector(kind, parent, &name),
        },
        name,
        kind,
        parent: parent.map(str::to_string),
        owner: None,
        arity: None,
        exported,
        interface_signature,
        range,
        clause_ranges: vec![range],
        calls: tree_sitter_calls(node, contents, language),
        relationships: Vec::new(),
    })
}

fn interface_signature_from_source(
    language: Language,
    symbol_kind: SymbolKind,
    name: &str,
    parent: Option<&str>,
    exported: bool,
    source: &str,
) -> InterfaceSignature {
    let qualified_name =
        parent.map_or_else(|| name.to_string(), |parent| format!("{parent}.{name}"));
    let visibility = exported.then(|| visibility_text(language, source));
    let (inputs, output) = match symbol_kind {
        SymbolKind::Function
        | SymbolKind::Method
        | SymbolKind::Macro
        | SymbolKind::Guard
        | SymbolKind::Callback
        | SymbolKind::MacroCallback => function_signature(language, source),
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
            aggregate_signature(language, source)
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
        language,
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
        if let Some(target) = function.and_then(|child| call_target(child, contents))
            && !is_call_keyword(&target, language)
        {
            calls.push(Call {
                target,
                line: node.start_position().row + 1,
            });
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
        Language::Elixir | Language::Unknown => false,
    }
}

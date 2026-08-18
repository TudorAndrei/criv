use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

use super::{
    FieldSignature, InterfaceSignature, Language, ModuleDecl, SourceFile, Symbol, SymbolId,
    SymbolKind, SymbolOwner, SymbolRange, elixir_symbol_selector, node_text,
};

#[derive(Clone)]
struct CallableClause {
    kind: SymbolKind,
    name: String,
    arity: usize,
    exported: bool,
    params: Vec<String>,
    guards: Vec<String>,
    defaults: Vec<String>,
    range: SymbolRange,
    has_body: bool,
}

#[derive(Clone)]
struct DefaultHead {
    kind: SymbolKind,
    name: String,
    full_arity: usize,
    default_count: usize,
    params: Vec<String>,
    defaults: Vec<String>,
    exported: bool,
}

#[derive(Clone)]
struct Specification {
    name: String,
    arity: usize,
    text: String,
    output: Option<String>,
}

struct Callback {
    kind: SymbolKind,
    name: String,
    arity: usize,
    params: Vec<String>,
    output: Option<String>,
    text: String,
    range: SymbolRange,
}

#[derive(Default)]
struct ModuleBody {
    callables: Vec<CallableClause>,
    default_heads: Vec<DefaultHead>,
    specifications: Vec<Specification>,
    callbacks: Vec<Callback>,
    optional_callbacks: BTreeSet<(String, usize)>,
    fields: Vec<FieldSignature>,
    has_struct: bool,
    has_exception: bool,
}

pub(super) fn parse_file(path: &str, contents: &str, root: Node<'_>) -> SourceFile {
    let mut file = SourceFile {
        path: path.into(),
        language: Language::Elixir,
        imports: Vec::new(),
        modules: Vec::new(),
        symbols: Vec::new(),
    };
    collect_modules(root, contents, path, None, &mut file);
    file
}

fn collect_modules(
    node: Node<'_>,
    contents: &str,
    path: &str,
    parent_module: Option<&str>,
    file: &mut SourceFile,
) {
    if declaration_target(node, contents)
        .as_deref()
        .is_some_and(is_module_declaration)
    {
        parse_module(node, contents, path, parent_module, file);
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_modules(child, contents, path, parent_module, file);
    }
}

fn parse_module(
    node: Node<'_>,
    contents: &str,
    path: &str,
    parent_module: Option<&str>,
    file: &mut SourceFile,
) {
    let Some(declaration) = declaration_target(node, contents) else {
        return;
    };
    let Some(arguments) = direct_child_kind(node, "arguments") else {
        return;
    };
    if node
        .child_by_field_name("target")
        .is_none_or(unsafe_subtree)
    {
        return;
    }
    let Some(first_argument) = first_named_child(arguments) else {
        return;
    };
    if unsafe_subtree(first_argument) {
        return;
    }

    let (owner, display_name, initial_kind) = if declaration == "defimpl" {
        let Some(protocol_text) = node_text(first_argument, contents) else {
            return;
        };
        let Some(protocol) = static_module_name(&protocol_text, parent_module) else {
            return;
        };
        let Some(for_node) = keyword_value(arguments, contents, "for:") else {
            return;
        };
        if unsafe_subtree(for_node) {
            return;
        }
        let Some(for_text) = node_text(for_node, contents) else {
            return;
        };
        let Some(for_type) = static_module_name(&for_text, parent_module) else {
            return;
        };
        (
            SymbolOwner::Implementation {
                protocol: protocol.clone(),
                for_type: for_type.clone(),
            },
            format!("{protocol} for {for_type}"),
            SymbolKind::Implementation,
        )
    } else {
        let Some(name_text) = node_text(first_argument, contents) else {
            return;
        };
        let Some(name) = static_module_name(&name_text, parent_module) else {
            return;
        };
        let kind = if declaration == "defprotocol" {
            SymbolKind::Protocol
        } else {
            SymbolKind::Module
        };
        (SymbolOwner::Module { name: name.clone() }, name, kind)
    };

    let range = node_range(node);
    let symbol_index = file.symbols.len();
    file.modules.push(ModuleDecl {
        name: display_name.clone(),
        line: range.start_line,
    });
    file.symbols.push(module_symbol(
        path,
        owner.clone(),
        display_name.clone(),
        initial_kind,
        range,
        Vec::new(),
    ));

    let mut body = ModuleBody::default();
    if let Some(do_block) =
        direct_child_kind(node, "do_block").or_else(|| keyword_value(arguments, contents, "do:"))
    {
        collect_module_body(
            do_block,
            contents,
            path,
            &display_name,
            initial_kind == SymbolKind::Protocol,
            file,
            &mut body,
        );
    }

    let final_kind = if initial_kind == SymbolKind::Protocol {
        SymbolKind::Protocol
    } else if initial_kind == SymbolKind::Implementation {
        SymbolKind::Implementation
    } else if body.has_exception {
        SymbolKind::Exception
    } else if body.has_struct {
        SymbolKind::Struct
    } else if !body.callbacks.is_empty() {
        SymbolKind::Behaviour
    } else {
        SymbolKind::Module
    };
    file.symbols[symbol_index] = module_symbol(
        path,
        owner.clone(),
        display_name.clone(),
        final_kind,
        range,
        body.fields.clone(),
    );

    emit_callables(path, &display_name, &owner, &body, file);
    emit_callbacks(path, &display_name, &owner, &body, file);
}

fn collect_module_body(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    allow_bodyless_function: bool,
    file: &mut SourceFile,
    body: &mut ModuleBody,
) {
    if declaration_target(node, contents)
        .as_deref()
        .is_some_and(is_module_declaration)
    {
        parse_module(node, contents, path, Some(module_name), file);
        return;
    }
    if let Some(target) = declaration_target(node, contents) {
        match target.as_str() {
            "def" | "defp" | "defmacro" | "defmacrop" | "defguard" | "defguardp"
            | "defdelegate" => {
                if !unsafe_subtree(node)
                    && let Some(clause) = callable_clause(node, contents, &target)
                {
                    if !clause.has_body
                        && clause.defaults.is_empty()
                        && matches!(clause.kind, SymbolKind::Function | SymbolKind::Macro)
                        && !allow_bodyless_function
                    {
                        return;
                    } else if !clause.has_body && !clause.defaults.is_empty() {
                        body.default_heads.push(DefaultHead {
                            kind: clause.kind,
                            name: clause.name,
                            full_arity: clause.arity,
                            default_count: clause.defaults.len(),
                            params: clause.params,
                            defaults: clause.defaults,
                            exported: clause.exported,
                        });
                    } else {
                        body.callables.push(clause);
                    }
                }
                return;
            }
            "defstruct" => {
                if !unsafe_subtree(node) {
                    body.has_struct = true;
                    body.fields.extend(struct_fields(node, contents));
                }
                return;
            }
            "defexception" => {
                if !unsafe_subtree(node) {
                    body.has_exception = true;
                    body.fields.extend(struct_fields(node, contents));
                }
                return;
            }
            _ => {}
        }
    }

    if node.kind() == "unary_operator"
        && operator_text(node, contents).as_deref() == Some("@")
        && !unsafe_subtree(node)
    {
        collect_attribute(node, contents, body);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_module_body(
            child,
            contents,
            path,
            module_name,
            allow_bodyless_function,
            file,
            body,
        );
    }
}

fn collect_attribute(node: Node<'_>, contents: &str, body: &mut ModuleBody) {
    let Some(call) = descendant_call(node) else {
        return;
    };
    let Some(target) = declaration_target(call, contents) else {
        return;
    };
    let Some(arguments) = direct_child_kind(call, "arguments") else {
        return;
    };
    let Some(value) = first_named_child(arguments) else {
        return;
    };
    let Some(text) = node_text(value, contents) else {
        return;
    };
    match target.as_str() {
        "spec" => {
            if let Some((name, arity, _params, output)) = signature_parts(&text) {
                body.specifications.push(Specification {
                    name,
                    arity,
                    text,
                    output,
                });
            }
        }
        "callback" | "macrocallback" => {
            if let Some((name, arity, params, output)) = signature_parts(&text) {
                body.callbacks.push(Callback {
                    kind: if target == "callback" {
                        SymbolKind::Callback
                    } else {
                        SymbolKind::MacroCallback
                    },
                    name,
                    arity,
                    params,
                    output,
                    text,
                    range: node_range(node),
                });
            }
        }
        "optional_callbacks" => {
            body.optional_callbacks.extend(optional_callbacks(&text));
        }
        _ => {}
    }
}

fn callable_clause(node: Node<'_>, contents: &str, declaration: &str) -> Option<CallableClause> {
    let arguments = direct_child_kind(node, "arguments")?;
    let mut head = first_named_child(arguments)?;
    let mut guards = Vec::new();
    if head.kind() == "binary_operator" && operator_text(head, contents).as_deref() == Some("when")
    {
        if let Some(guard) = head
            .child_by_field_name("right")
            .and_then(|node| node_text(node, contents))
        {
            guards.push(guard);
        }
        head = head.child_by_field_name("left")?;
    }
    let (name, params) = callable_head(head, contents)?;
    let defaults = params
        .iter()
        .filter(|parameter| parameter.contains("\\\\"))
        .cloned()
        .collect::<Vec<_>>();
    let kind = match declaration {
        "def" | "defp" | "defdelegate" => SymbolKind::Function,
        "defmacro" | "defmacrop" => SymbolKind::Macro,
        "defguard" | "defguardp" => SymbolKind::Guard,
        _ => return None,
    };
    let exported = !matches!(declaration, "defp" | "defmacrop" | "defguardp");
    Some(CallableClause {
        kind,
        name,
        arity: params.len(),
        exported,
        params,
        guards,
        defaults,
        range: node_range(node),
        has_body: direct_child_kind(node, "do_block").is_some()
            || keyword_value(arguments, contents, "do:").is_some()
            || declaration == "defdelegate",
    })
}

fn callable_head(node: Node<'_>, contents: &str) -> Option<(String, Vec<String>)> {
    match node.kind() {
        "call" => {
            let target = node.child_by_field_name("target")?;
            let name = node_text(target, contents)?;
            let params = direct_child_kind(node, "arguments")
                .map(|arguments| named_children_text(arguments, contents))
                .unwrap_or_default();
            Some((normalize_callable_name(&name), params))
        }
        "identifier" | "operator_identifier" => Some((
            normalize_callable_name(&node_text(node, contents)?),
            Vec::new(),
        )),
        "binary_operator" => {
            let name = operator_text(node, contents)?;
            if matches!(name.as_str(), "when" | "\\\\" | "::" | "|>") {
                return None;
            }
            let left = node
                .child_by_field_name("left")
                .and_then(|node| node_text(node, contents))?;
            let right = node
                .child_by_field_name("right")
                .and_then(|node| node_text(node, contents))?;
            Some((name, vec![left, right]))
        }
        "unary_operator" => {
            let name = operator_text(node, contents)?;
            if name == "@" {
                return None;
            }
            let operand = node
                .child_by_field_name("operand")
                .or_else(|| first_named_child(node))
                .and_then(|node| node_text(node, contents))?;
            Some((name, vec![operand]))
        }
        _ => None,
    }
}

fn emit_callables(
    path: &str,
    module_name: &str,
    owner: &SymbolOwner,
    body: &ModuleBody,
    file: &mut SourceFile,
) {
    let mut groups = BTreeMap::<(SymbolKind, String, usize), Vec<CallableClause>>::new();
    for clause in &body.callables {
        let default_count = clause.defaults.len();
        let first_arity = clause.arity.saturating_sub(default_count);
        for arity in first_arity..=clause.arity {
            groups
                .entry((clause.kind, clause.name.clone(), arity))
                .or_default()
                .push(clause.clone());
        }
    }
    for head in &body.default_heads {
        let source_clauses = groups
            .get(&(head.kind, head.name.clone(), head.full_arity))
            .cloned()
            .unwrap_or_default();
        if source_clauses.is_empty() {
            continue;
        }
        for arity in head.full_arity.saturating_sub(head.default_count)..head.full_arity {
            groups
                .entry((head.kind, head.name.clone(), arity))
                .or_insert_with(|| source_clauses.clone());
        }
    }

    for ((kind, name, arity), mut clauses) in groups {
        clauses.sort_by_key(|clause| (clause.range.start_line, clause.range.end_line));
        let ranges = clauses
            .iter()
            .map(|clause| clause.range)
            .collect::<Vec<_>>();
        let exported = clauses.iter().any(|clause| clause.exported)
            || body
                .default_heads
                .iter()
                .any(|head| head.kind == kind && head.name == name && head.exported);
        let mut defaults = clauses
            .iter()
            .flat_map(|clause| clause.defaults.clone())
            .collect::<Vec<_>>();
        for head in body
            .default_heads
            .iter()
            .filter(|head| head.kind == kind && head.name == name)
        {
            defaults.extend(head.defaults.clone());
        }
        defaults.dedup();
        let specifications = body
            .specifications
            .iter()
            .filter(|spec| spec.name == name && spec.arity == arity)
            .map(|spec| spec.text.clone())
            .collect::<Vec<_>>();
        let output = body
            .specifications
            .iter()
            .find(|spec| spec.name == name && spec.arity == arity)
            .and_then(|spec| spec.output.clone());
        let inputs = clauses
            .iter()
            .enumerate()
            .map(|(index, clause)| format!("clause {}: {}", index + 1, clause.params.join(", ")))
            .chain(
                body.default_heads
                    .iter()
                    .filter(|head| head.kind == kind && head.name == name)
                    .map(|head| format!("default head: {}", head.params.join(", "))),
            )
            .collect::<Vec<_>>();
        let guards = clauses
            .iter()
            .flat_map(|clause| clause.guards.clone())
            .collect::<Vec<_>>();
        let range = ranges[0];
        let selector = elixir_symbol_selector(kind, owner, &name, Some(arity))
            .expect("Elixir callable selector");
        file.symbols.push(Symbol {
            id: SymbolId {
                path: path.into(),
                name: name.clone(),
                selector,
            },
            name: name.clone(),
            kind,
            parent: None,
            owner: Some(owner.clone()),
            arity: Some(arity),
            exported,
            interface_signature: Some(InterfaceSignature {
                language: Language::Elixir,
                symbol_kind: kind,
                qualified_name: format!("{module_name}.{name}/{arity}"),
                visibility: Some(if exported { "public" } else { "private" }.into()),
                inputs,
                output,
                fields: Vec::new(),
                variants: Vec::new(),
                arity: Some(arity),
                guards,
                defaults,
                specifications,
            }),
            range,
            clause_ranges: ranges,
            calls: Vec::new(),
        });
    }
}

fn emit_callbacks(
    path: &str,
    module_name: &str,
    owner: &SymbolOwner,
    body: &ModuleBody,
    file: &mut SourceFile,
) {
    for callback in &body.callbacks {
        let selector =
            elixir_symbol_selector(callback.kind, owner, &callback.name, Some(callback.arity))
                .expect("Elixir callback selector");
        let optional = body
            .optional_callbacks
            .contains(&(callback.name.clone(), callback.arity));
        file.symbols.push(Symbol {
            id: SymbolId {
                path: path.into(),
                name: callback.name.clone(),
                selector,
            },
            name: callback.name.clone(),
            kind: callback.kind,
            parent: None,
            owner: Some(owner.clone()),
            arity: Some(callback.arity),
            exported: true,
            interface_signature: Some(InterfaceSignature {
                language: Language::Elixir,
                symbol_kind: callback.kind,
                qualified_name: format!("{module_name}.{}/{}", callback.name, callback.arity),
                visibility: Some(if optional { "optional" } else { "public" }.into()),
                inputs: callback.params.clone(),
                output: callback.output.clone(),
                fields: Vec::new(),
                variants: Vec::new(),
                arity: Some(callback.arity),
                guards: Vec::new(),
                defaults: Vec::new(),
                specifications: vec![callback.text.clone()],
            }),
            range: callback.range,
            clause_ranges: vec![callback.range],
            calls: Vec::new(),
        });
    }
}

fn module_symbol(
    path: &str,
    owner: SymbolOwner,
    name: String,
    kind: SymbolKind,
    range: SymbolRange,
    mut fields: Vec<FieldSignature>,
) -> Symbol {
    fields.sort();
    fields.dedup();
    let selector =
        elixir_symbol_selector(kind, &owner, &name, None).expect("Elixir module selector");
    Symbol {
        id: SymbolId {
            path: path.into(),
            name: name.clone(),
            selector,
        },
        name: name.clone(),
        kind,
        parent: None,
        owner: Some(owner),
        arity: None,
        exported: true,
        interface_signature: Some(InterfaceSignature {
            language: Language::Elixir,
            symbol_kind: kind,
            qualified_name: name,
            visibility: Some("public".into()),
            inputs: Vec::new(),
            output: None,
            fields,
            variants: Vec::new(),
            arity: None,
            guards: Vec::new(),
            defaults: Vec::new(),
            specifications: Vec::new(),
        }),
        range,
        clause_ranges: vec![range],
        calls: Vec::new(),
    }
}

fn declaration_target(node: Node<'_>, contents: &str) -> Option<String> {
    (node.kind() == "call")
        .then(|| node.child_by_field_name("target"))
        .flatten()
        .and_then(|target| node_text(target, contents))
}

fn is_module_declaration(target: &str) -> bool {
    matches!(target, "defmodule" | "defprotocol" | "defimpl")
}

fn unsafe_subtree(node: Node<'_>) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(unsafe_subtree)
}

fn static_module_name(text: &str, parent: Option<&str>) -> Option<String> {
    let text = text.trim();
    if text == "__MODULE__" {
        return parent.map(str::to_string);
    }
    if let Some(child) = text.strip_prefix("__MODULE__.") {
        return parent.map(|parent| format!("{parent}.{child}"));
    }
    if let Some(name) = text
        .strip_prefix(":\"Elixir.")
        .and_then(|name| name.strip_suffix('"'))
        .or_else(|| text.strip_prefix("Elixir."))
    {
        return static_alias(name).then(|| name.to_string());
    }
    if text.starts_with(':') {
        return static_atom(text).then(|| text.to_string());
    }
    if !static_alias(text) {
        return None;
    }
    Some(parent.map_or_else(|| text.to_string(), |parent| format!("{parent}.{text}")))
}

fn static_alias(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            part.chars().next().is_some_and(char::is_uppercase)
                && part
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
        })
}

fn static_atom(value: &str) -> bool {
    let atom = value.trim_start_matches(':').trim_matches('"');
    !atom.is_empty()
        && atom.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '.' | '!' | '?')
        })
}

fn keyword_value<'tree>(
    node: Node<'tree>,
    contents: &str,
    wanted_key: &str,
) -> Option<Node<'tree>> {
    if node.kind() == "pair"
        && node
            .child_by_field_name("key")
            .and_then(|key| node_text(key, contents))
            .is_some_and(|key| {
                key.trim().trim_end_matches(':') == wanted_key.trim().trim_end_matches(':')
            })
    {
        return node.child_by_field_name("value");
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| keyword_value(child, contents, wanted_key))
}

fn operator_text(node: Node<'_>, contents: &str) -> Option<String> {
    node.child_by_field_name("operator")
        .and_then(|operator| node_text(operator, contents))
}

fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn direct_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn named_children_text(node: Node<'_>, contents: &str) -> Vec<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter_map(|child| node_text(child, contents))
        .collect()
}

fn descendant_call(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "call" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(descendant_call)
}

fn node_range(node: Node<'_>) -> SymbolRange {
    SymbolRange {
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    }
}

fn normalize_callable_name(name: &str) -> String {
    name.trim().trim_matches(':').trim_matches('"').to_string()
}

fn signature_parts(text: &str) -> Option<(String, usize, Vec<String>, Option<String>)> {
    let (head, output) = split_top_level_operator(text, "::")
        .map(|(left, right)| (left.trim(), Some(right.trim().to_string())))
        .unwrap_or((text.trim(), None));
    let head = split_top_level_operator(head, "when")
        .map(|(left, _)| left.trim())
        .unwrap_or(head);
    if let Some(open) = head.find('(') {
        let close = matching_close(head, open, '(', ')')?;
        let name = normalize_callable_name(head[..open].trim());
        let params = split_top_level(&head[open + 1..close], ',');
        return (!name.is_empty()).then_some((name, params.len(), params, output));
    }
    let name = normalize_callable_name(head);
    (!name.is_empty()).then_some((name, 0, Vec::new(), output))
}

fn split_top_level_operator<'a>(value: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    let mut depths = [0usize; 4];
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index + operator.len() <= bytes.len() {
        let character = bytes[index] as char;
        match character {
            '(' => depths[0] += 1,
            ')' => depths[0] = depths[0].saturating_sub(1),
            '[' => depths[1] += 1,
            ']' => depths[1] = depths[1].saturating_sub(1),
            '{' => depths[2] += 1,
            '}' => depths[2] = depths[2].saturating_sub(1),
            '<' => depths[3] += 1,
            '>' => depths[3] = depths[3].saturating_sub(1),
            _ => {}
        }
        if depths == [0; 4] && value[index..].starts_with(operator) {
            return Some((&value[..index], &value[index + operator.len()..]));
        }
        index += 1;
    }
    None
}

fn matching_close(value: &str, open: usize, open_char: char, close_char: char) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in value[open..].char_indices() {
        if character == open_char {
            depth += 1;
        } else if character == close_char {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

fn split_top_level(value: &str, separator: char) -> Vec<String> {
    let mut rows = Vec::new();
    let mut depths = [0usize; 4];
    let mut start = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depths[0] += 1,
            ')' => depths[0] = depths[0].saturating_sub(1),
            '[' => depths[1] += 1,
            ']' => depths[1] = depths[1].saturating_sub(1),
            '{' => depths[2] += 1,
            '}' => depths[2] = depths[2].saturating_sub(1),
            '<' => depths[3] += 1,
            '>' => depths[3] = depths[3].saturating_sub(1),
            _ => {}
        }
        if character == separator && depths == [0; 4] {
            let row = value[start..index].trim();
            if !row.is_empty() {
                rows.push(row.to_string());
            }
            start = index + character.len_utf8();
        }
    }
    let row = value[start..].trim();
    if !row.is_empty() {
        rows.push(row.to_string());
    }
    rows
}

fn struct_fields(node: Node<'_>, contents: &str) -> Vec<FieldSignature> {
    let Some(arguments) = direct_child_kind(node, "arguments") else {
        return Vec::new();
    };
    let Some(value) = first_named_child(arguments).and_then(|node| node_text(node, contents))
    else {
        return Vec::new();
    };
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    split_top_level(value, ',')
        .into_iter()
        .filter_map(|field| {
            let name = field
                .strip_prefix(':')
                .or_else(|| field.split_once(':').map(|(name, _)| name))?
                .trim()
                .trim_matches('"')
                .to_string();
            (!name.is_empty()).then_some(FieldSignature { name, ty: None })
        })
        .collect()
}

fn optional_callbacks(text: &str) -> BTreeSet<(String, usize)> {
    let value = text.trim().trim_start_matches('[').trim_end_matches(']');
    split_top_level(value, ',')
        .into_iter()
        .filter_map(|entry| {
            let (name, arity) = entry.split_once(':')?;
            Some((
                normalize_callable_name(name.trim()),
                arity.trim().parse().ok()?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tree_sitter::Parser;

    use super::*;

    fn parse(path: &str, source: &str) -> SourceFile {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_elixir::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        parse_file(path, source, tree.root_node())
    }

    #[test]
    fn extracts_module_kinds_callables_defaults_clauses_and_interfaces() {
        let file = parse(
            "lib/sample.ex",
            r#"
defmodule My.App do
  @spec run(term(), keyword()) :: {:ok, term()}
  defstruct [:id, name: nil]

  def run(value, opts \\ [])
  def run(:skip, opts), do: {:ok, opts}
  def run(value, opts) when is_list(opts), do: {:ok, value}
  defp hidden, do: :ok
  defmacro build(value), do: value
  defmacrop secret(value), do: value
  defguard valid(value) when is_integer(value)
  defguardp local_valid(value) when is_atom(value)
  defdelegate fetch(id), to: Repo
  def left + right, do: left + right
  def zero, do: :ok

  defmodule Child do
    def child(), do: :ok
  end
end

defprotocol Renderable do
  def render(value)
end

defimpl Renderable, for: My.App do
  def render(value), do: inspect(value)
end

defmodule Failure do
  defexception [:message]
end

defmodule WorkerBehaviour do
  @callback run(term()) :: :ok
  @macrocallback build(term()) :: Macro.t()
  @optional_callbacks [run: 1, build: 1]
end

defmodule Inline, do: (def ready(), do: :ok)
"#,
        );

        let symbols = &file.symbols;
        assert!(symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Struct
                && symbol.id.selector == "module:My.App"
                && symbol
                    .interface_signature
                    .as_ref()
                    .is_some_and(|signature| signature.fields.len() == 2)
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Module && symbol.id.selector == "module:My.App.Child"
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Protocol && symbol.id.selector == "module:Renderable"
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Implementation
                && symbol.id.selector == "impl:Renderable/for:My.App"
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Exception && symbol.id.selector == "module:Failure"
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Behaviour && symbol.id.selector == "module:WorkerBehaviour"
        }));

        let run_one = symbols
            .iter()
            .find(|symbol| symbol.id.selector == "module:My.App/fn:run/1")
            .unwrap();
        let run_two = symbols
            .iter()
            .find(|symbol| symbol.id.selector == "module:My.App/fn:run/2")
            .unwrap();
        assert_eq!(run_one.clause_ranges.len(), 2);
        assert_eq!(run_two.clause_ranges.len(), 2);
        assert_eq!(run_two.range, run_two.clause_ranges[0]);
        let signature = run_two.interface_signature.as_ref().unwrap();
        assert_eq!(signature.arity, Some(2));
        assert_eq!(signature.guards, vec!["is_list(opts)"]);
        assert_eq!(signature.specifications.len(), 1);
        assert!(!signature.defaults.is_empty());

        for selector in [
            "module:My.App/fn:hidden/0",
            "module:My.App/macro:build/1",
            "module:My.App/macro:secret/1",
            "module:My.App/guard:valid/1",
            "module:My.App/guard:local_valid/1",
            "module:My.App/fn:fetch/1",
            "module:My.App/fn:%2B/2",
            "module:My.App/fn:zero/0",
            "module:My.App.Child/fn:child/0",
            "module:WorkerBehaviour/callback:run/1",
            "module:WorkerBehaviour/macro-callback:build/1",
            "module:Inline/fn:ready/0",
        ] {
            assert!(
                symbols.iter().any(|symbol| symbol.id.selector == selector),
                "missing {selector}"
            );
        }
        assert!(
            !symbols
                .iter()
                .find(|symbol| symbol.id.selector == "module:My.App/fn:hidden/0")
                .unwrap()
                .exported
        );
        for selector in [
            "module:My.App/macro:secret/1",
            "module:My.App/guard:local_valid/1",
        ] {
            assert!(
                !symbols
                    .iter()
                    .find(|symbol| symbol.id.selector == selector)
                    .unwrap()
                    .exported
            );
        }
        assert_eq!(
            symbols
                .iter()
                .find(|symbol| { symbol.id.selector == "module:WorkerBehaviour/callback:run/1" })
                .unwrap()
                .interface_signature
                .as_ref()
                .unwrap()
                .visibility
                .as_deref(),
            Some("optional")
        );
    }

    #[test]
    fn skips_dynamic_modules_and_unsafe_declarations_but_keeps_safe_siblings() {
        let file = parse(
            "lib/partial.ex",
            r#"
defmodule Before do
  def ok(), do: :ok
  def broken(, do: :bad)
  def after_error(), do: :ok

  defmodule Module.concat([Dynamic, Child]) do
    def nested_skipped(), do: :ok
  end
end

defmodule Module.concat([Dynamic, Name]) do
  def skipped(), do: :ok
end

defmodule After do
  def ok(), do: :ok
end
"#,
        );

        assert!(
            file.symbols
                .iter()
                .any(|symbol| symbol.id.selector == "module:Before")
        );
        assert!(
            file.symbols
                .iter()
                .any(|symbol| symbol.id.selector == "module:After/fn:ok/0")
        );
        assert!(
            file.symbols
                .iter()
                .any(|symbol| symbol.id.selector == "module:Before/fn:after_error/0")
        );
        assert!(!file.symbols.iter().any(|symbol| symbol.name == "skipped"));
        assert!(
            !file
                .symbols
                .iter()
                .any(|symbol| symbol.name == "nested_skipped")
        );
        let broken = file.symbols.iter().find(|symbol| symbol.name == "broken");
        assert!(
            broken.is_none(),
            "unexpected recovered declaration: {broken:?}"
        );
    }

    #[test]
    fn interface_hash_ignores_bodies_and_tracks_elixir_contract_changes() {
        let before = parse(
            "lib/sample.ex",
            r#"
defmodule Sample do
  @spec run(term()) :: :ok
  def run(value) when is_atom(value), do: value
end
"#,
        );
        let body_change = parse(
            "lib/sample.ex",
            r#"
defmodule Sample do
  @spec run(term()) :: :ok
  def run(value) when is_atom(value), do: {:changed, value}
end
"#,
        );
        let contract_change = parse(
            "lib/sample.ex",
            r#"
defmodule Sample do
  @spec run(term()) :: {:ok, term()}
  def run(value) when is_binary(value), do: value
end
"#,
        );
        let hash = |file: &SourceFile| {
            file.symbols
                .iter()
                .find(|symbol| symbol.name == "run")
                .unwrap()
                .interface_signature
                .as_ref()
                .unwrap()
                .hash()
        };

        assert_eq!(hash(&before), hash(&body_change));
        assert_ne!(hash(&before), hash(&contract_change));
    }
}

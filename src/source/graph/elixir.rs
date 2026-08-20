use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

use super::{
    CallableFilter, DirectiveFilter, DirectiveKind, FieldSignature, Import, InterfaceSignature,
    Language, LexicalScope, ModuleDecl, ModuleRelationshipRole, Relationship, RelationshipKind,
    RelationshipTarget, SourceFile, Symbol, SymbolId, SymbolKind, SymbolOwner, SymbolRange,
    elixir_symbol_selector, node_text,
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
    relationships: Vec<Relationship>,
    default_relationships: Vec<Relationship>,
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
    default_relationships: Vec<Relationship>,
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
    relationships: Vec<Relationship>,
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
    let mut module_relationships = match &owner {
        SymbolOwner::Implementation { protocol, for_type } => vec![
            Relationship {
                kind: RelationshipKind::ProtocolImplementation,
                target: RelationshipTarget::Module {
                    module: protocol.clone(),
                    role: ModuleRelationshipRole::Protocol,
                },
                line: range.start_line,
                site: node.start_byte(),
            },
            Relationship {
                kind: RelationshipKind::ProtocolImplementation,
                target: RelationshipTarget::Module {
                    module: for_type.clone(),
                    role: ModuleRelationshipRole::ForType,
                },
                line: range.start_line,
                site: node.start_byte(),
            },
        ],
        SymbolOwner::Module { .. } => Vec::new(),
    };
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
        module_relationships.clone(),
    ));

    let mut body = ModuleBody::default();
    if let Some(do_block) =
        direct_child_kind(node, "do_block").or_else(|| keyword_value(arguments, contents, "do:"))
    {
        collect_directives(
            do_block,
            contents,
            &display_name,
            &owner,
            lexical_scope(do_block),
            file,
        );
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
    module_relationships.extend(body.relationships.clone());
    file.symbols[symbol_index] = module_symbol(
        path,
        owner.clone(),
        display_name.clone(),
        final_kind,
        range,
        body.fields.clone(),
        module_relationships,
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
                    && let Some(clause) =
                        callable_clause(node, contents, path, module_name, &target)
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
                            default_relationships: clause.default_relationships,
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
        collect_attribute(node, contents, module_name, body);
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

fn collect_directives(
    node: Node<'_>,
    contents: &str,
    module_name: &str,
    owner: &SymbolOwner,
    scope: LexicalScope,
    file: &mut SourceFile,
) {
    if declaration_target(node, contents)
        .as_deref()
        .is_some_and(is_module_declaration)
    {
        return;
    }
    if let Some(target) = declaration_target(node, contents)
        && let Some(kind) = directive_kind(&target)
    {
        if !unsafe_subtree(node) {
            file.imports.extend(parse_directive(
                node,
                contents,
                module_name,
                owner,
                scope,
                kind,
            ));
        }
        return;
    }

    let child_scope = if matches!(node.kind(), "do_block" | "stab_clause")
        || is_lexical_keyword_pair(node, contents)
    {
        lexical_scope(node)
    } else {
        scope
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_directives(child, contents, module_name, owner, child_scope, file);
    }
}

fn is_lexical_keyword_pair(node: Node<'_>, contents: &str) -> bool {
    node.kind() == "pair"
        && node
            .child_by_field_name("key")
            .and_then(|key| node_text(key, contents))
            .is_some_and(|key| {
                matches!(
                    key.trim().trim_end_matches(':'),
                    "do" | "else" | "rescue" | "catch" | "after"
                )
            })
}

fn directive_kind(target: &str) -> Option<DirectiveKind> {
    match target {
        "alias" => Some(DirectiveKind::Alias),
        "import" => Some(DirectiveKind::Import),
        "require" => Some(DirectiveKind::Require),
        "use" => Some(DirectiveKind::Use),
        _ => None,
    }
}

fn parse_directive(
    node: Node<'_>,
    contents: &str,
    module_name: &str,
    owner: &SymbolOwner,
    scope: LexicalScope,
    kind: DirectiveKind,
) -> Vec<Import> {
    let Some(arguments) = direct_child_kind(node, "arguments") else {
        return Vec::new();
    };
    let Some(module_text) = first_named_child(arguments).and_then(|node| node_text(node, contents))
    else {
        return Vec::new();
    };
    let explicit_alias = keyword_value(arguments, contents, "as:")
        .and_then(|node| node_text(node, contents))
        .map(|value| normalize_alias_name(&value));
    let only = keyword_value(arguments, contents, "only:")
        .and_then(|node| node_text(node, contents))
        .and_then(|value| parse_directive_filter(&value));
    let except = keyword_value(arguments, contents, "except:")
        .and_then(|node| node_text(node, contents))
        .map(|value| parse_callable_filters(&value))
        .unwrap_or_default();
    let absolute = is_explicit_elixir_module(&module_text);
    let modules = expand_reference_modules(&module_text, module_name);
    let one_module = modules.len() == 1;
    modules
        .into_iter()
        .map(|module| {
            let alias = if one_module {
                explicit_alias.clone().or_else(|| match kind {
                    DirectiveKind::Alias => module
                        .rsplit('.')
                        .next()
                        .map(str::to_string)
                        .filter(|value| !value.starts_with(':')),
                    DirectiveKind::Import
                    | DirectiveKind::Require
                    | DirectiveKind::Use
                    | DirectiveKind::Legacy => None,
                })
            } else if kind == DirectiveKind::Alias {
                module.rsplit('.').next().map(str::to_string)
            } else {
                None
            };
            Import {
                module,
                line: node.start_position().row + 1,
                site: node.start_byte(),
                kind,
                owner: Some(owner.clone()),
                scope: Some(scope),
                alias,
                only: only.clone(),
                except: except.clone(),
                absolute,
            }
        })
        .collect()
}

fn expand_reference_modules(text: &str, module_name: &str) -> Vec<String> {
    let text = text.trim();
    if let Some((prefix, suffix)) = text.split_once(".{")
        && let Some(inner) = suffix.strip_suffix('}')
    {
        return split_top_level(inner, ',')
            .into_iter()
            .filter_map(|part| static_reference_module(&format!("{prefix}.{part}"), module_name))
            .collect();
    }
    static_reference_module(text, module_name)
        .into_iter()
        .collect()
}

fn normalize_alias_name(value: &str) -> String {
    value
        .trim()
        .trim_start_matches(':')
        .trim_matches('"')
        .to_string()
}

fn is_explicit_elixir_module(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("Elixir.") || value.starts_with(":\"Elixir.")
}

fn parse_directive_filter(value: &str) -> Option<DirectiveFilter> {
    match value.trim() {
        ":functions" => Some(DirectiveFilter::Functions),
        ":macros" => Some(DirectiveFilter::Macros),
        ":sigils" => Some(DirectiveFilter::Sigils),
        value => {
            let filters = parse_callable_filters(value);
            (!filters.is_empty()).then_some(DirectiveFilter::Callables(filters))
        }
    }
}

fn parse_callable_filters(value: &str) -> Vec<CallableFilter> {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    split_top_level(value, ',')
        .into_iter()
        .filter_map(|entry| {
            let (name, arity) = entry.split_once(':')?;
            Some(CallableFilter {
                name: normalize_callable_name(name.trim()),
                arity: arity.trim().parse().ok()?,
            })
        })
        .collect()
}

fn collect_attribute(node: Node<'_>, contents: &str, module_name: &str, body: &mut ModuleBody) {
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
        "behaviour" => {
            if let Some(module) = static_relationship_module(&text, module_name) {
                body.relationships.push(Relationship {
                    kind: RelationshipKind::BehaviourImplementation,
                    target: RelationshipTarget::Module {
                        module,
                        role: ModuleRelationshipRole::Behaviour,
                    },
                    line: node.start_position().row + 1,
                    site: node.start_byte(),
                });
            }
        }
        _ => {}
    }
}

fn callable_clause(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    declaration: &str,
) -> Option<CallableClause> {
    let arguments = direct_child_kind(node, "arguments")?;
    let mut head = first_named_child(arguments)?;
    let mut guards = Vec::new();
    let mut relationships = Vec::new();
    if head.kind() == "binary_operator" && operator_text(head, contents).as_deref() == Some("when")
    {
        if let Some(guard_node) = head.child_by_field_name("right") {
            if let Some(guard) = node_text(guard_node, contents) {
                guards.push(guard);
            }
            collect_expression_relationships(
                guard_node,
                contents,
                path,
                module_name,
                &mut relationships,
            );
        }
        head = head.child_by_field_name("left")?;
    }
    let (name, params) = callable_head(head, contents)?;
    let mut default_relationships = Vec::new();
    collect_default_relationships(
        head,
        contents,
        path,
        module_name,
        &mut default_relationships,
    );
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
    if declaration == "defdelegate" {
        relationships.push(delegate_relationship(
            node,
            contents,
            path,
            module_name,
            &name,
            params.len(),
        ));
    } else if let Some(body_node) =
        direct_child_kind(node, "do_block").or_else(|| keyword_value(arguments, contents, "do:"))
    {
        collect_expression_relationships(
            body_node,
            contents,
            path,
            module_name,
            &mut relationships,
        );
    }
    relationships.sort();
    relationships.dedup();
    default_relationships.sort();
    default_relationships.dedup();
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
        relationships,
        default_relationships,
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

fn collect_default_relationships(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    relationships: &mut Vec<Relationship>,
) {
    if node.kind() == "binary_operator" && operator_text(node, contents).as_deref() == Some("\\\\")
    {
        if let Some(default) = node.child_by_field_name("right") {
            collect_expression_relationships(default, contents, path, module_name, relationships);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_default_relationships(child, contents, path, module_name, relationships);
    }
}

fn collect_expression_relationships(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    relationships: &mut Vec<Relationship>,
) {
    if node.kind() == "unary_operator"
        && operator_text(node, contents).as_deref() == Some("&")
        && let Some(capture) = capture_relationship(node, contents, path, module_name)
    {
        relationships.push(capture);
        return;
    }

    if node.kind() == "binary_operator" && operator_text(node, contents).as_deref() == Some("|>") {
        if let Some(left) = node.child_by_field_name("left") {
            collect_expression_relationships(left, contents, path, module_name, relationships);
        }
        if let Some(right) = node.child_by_field_name("right") {
            if right.kind() == "call" {
                collect_call_relationship(right, contents, path, module_name, 1, relationships);
            } else {
                collect_expression_relationships(right, contents, path, module_name, relationships);
            }
        }
        return;
    }

    if node.kind() == "binary_operator"
        && let Some(operator) = operator_text(node, contents)
        && !matches!(operator.as_str(), "when" | "\\\\" | "::" | ".")
    {
        relationships.push(Relationship {
            kind: RelationshipKind::Call,
            target: RelationshipTarget::Callable {
                module: None,
                name: operator,
                arity: 2,
            },
            line: node.start_position().row + 1,
            site: node.start_byte(),
        });
        for field in ["left", "right"] {
            if let Some(child) = node.child_by_field_name(field) {
                collect_expression_relationships(child, contents, path, module_name, relationships);
            }
        }
        return;
    }

    if node.kind() == "unary_operator"
        && let Some(operator) = operator_text(node, contents)
        && !matches!(operator.as_str(), "&" | "@")
    {
        relationships.push(Relationship {
            kind: RelationshipKind::Call,
            target: RelationshipTarget::Callable {
                module: None,
                name: operator,
                arity: 1,
            },
            line: node.start_position().row + 1,
            site: node.start_byte(),
        });
        if let Some(operand) = node
            .child_by_field_name("operand")
            .or_else(|| first_named_child(node))
        {
            collect_expression_relationships(operand, contents, path, module_name, relationships);
        }
        return;
    }

    if node.kind() == "call" {
        collect_call_relationship(node, contents, path, module_name, 0, relationships);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_expression_relationships(child, contents, path, module_name, relationships);
    }
}

fn collect_call_relationship(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    pipeline_arguments: usize,
    relationships: &mut Vec<Relationship>,
) {
    let Some(target_node) = node.child_by_field_name("target") else {
        return;
    };
    let target_text = node_text(target_node, contents).unwrap_or_default();
    if is_excluded_call_tree(&target_text) {
        return;
    }
    let arguments = direct_child_kind(node, "arguments");
    let arity = arguments.map_or(0, named_child_count) + pipeline_arguments;

    if target_text == "apply" && pipeline_arguments == 0 {
        relationships.push(apply_relationship(node, contents, path, module_name));
    } else if !is_elixir_special_form(&target_text) {
        relationships.push(Relationship {
            kind: RelationshipKind::Call,
            target: callable_relationship_target(
                target_node,
                contents,
                path,
                module_name,
                arity,
                node,
            ),
            line: node.start_position().row + 1,
            site: node.start_byte(),
        });
    }

    if let Some(arguments) = arguments {
        let mut cursor = arguments.walk();
        for argument in arguments.named_children(&mut cursor) {
            collect_expression_relationships(argument, contents, path, module_name, relationships);
        }
    }
    if let Some(do_block) = direct_child_kind(node, "do_block") {
        collect_expression_relationships(do_block, contents, path, module_name, relationships);
    }
}

fn callable_relationship_target(
    target: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    arity: usize,
    call_site: Node<'_>,
) -> RelationshipTarget {
    match target.kind() {
        "identifier" | "operator_identifier" => RelationshipTarget::Callable {
            module: None,
            name: normalize_callable_name(&node_text(target, contents).unwrap_or_default()),
            arity,
        },
        "dot" => {
            let left = target.child_by_field_name("left");
            let right = target.child_by_field_name("right");
            match (left, right) {
                (Some(left), Some(right)) => {
                    let name =
                        normalize_callable_name(&node_text(right, contents).unwrap_or_default());
                    let module = node_text(left, contents)
                        .and_then(|value| static_relationship_module(&value, module_name));
                    match module {
                        Some(module) => RelationshipTarget::Callable {
                            module: Some(module),
                            name,
                            arity,
                        },
                        None => dynamic_target(
                            path,
                            call_site,
                            format!("dynamic.{name}/{arity}"),
                            arity,
                        ),
                    }
                }
                _ => dynamic_target(path, call_site, format!("anonymous/{arity}"), arity),
            }
        }
        _ => dynamic_target(path, call_site, format!("dynamic/{arity}"), arity),
    }
}

fn capture_relationship(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
) -> Option<Relationship> {
    let operand = node
        .child_by_field_name("operand")
        .or_else(|| first_named_child(node))?;
    if operand.kind() != "binary_operator"
        || operator_text(operand, contents).as_deref() != Some("/")
    {
        return None;
    }
    let target = operand.child_by_field_name("left")?;
    let arity = operand
        .child_by_field_name("right")
        .and_then(|node| node_text(node, contents))?
        .parse::<usize>()
        .ok()?;
    let target = if target.kind() == "call" {
        callable_relationship_target(
            target.child_by_field_name("target")?,
            contents,
            path,
            module_name,
            arity,
            node,
        )
    } else {
        callable_relationship_target(target, contents, path, module_name, arity, node)
    };
    Some(Relationship {
        kind: RelationshipKind::Capture,
        target,
        line: node.start_position().row + 1,
        site: node.start_byte(),
    })
}

fn delegate_relationship(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
    source_name: &str,
    arity: usize,
) -> Relationship {
    let arguments = direct_child_kind(node, "arguments");
    let target_name = arguments
        .and_then(|arguments| keyword_value(arguments, contents, "as:"))
        .and_then(|node| node_text(node, contents))
        .map(|value| normalize_callable_name(&value))
        .unwrap_or_else(|| source_name.to_string());
    let module = arguments
        .and_then(|arguments| keyword_value(arguments, contents, "to:"))
        .and_then(|node| node_text(node, contents))
        .and_then(|value| static_relationship_module(&value, module_name));
    let target = match module {
        Some(module) => RelationshipTarget::Callable {
            module: Some(module),
            name: target_name,
            arity,
        },
        None => dynamic_target(path, node, format!("delegate.{target_name}/{arity}"), arity),
    };
    Relationship {
        kind: RelationshipKind::Delegate,
        target,
        line: node.start_position().row + 1,
        site: node.start_byte(),
    }
}

fn apply_relationship(
    node: Node<'_>,
    contents: &str,
    path: &str,
    module_name: &str,
) -> Relationship {
    let arguments = direct_child_kind(node, "arguments")
        .map(|arguments| named_children(arguments))
        .unwrap_or_default();
    let target = if let [module, function, arguments] = arguments.as_slice() {
        let module = node_text(*module, contents)
            .and_then(|value| static_relationship_module(&value, module_name));
        let function =
            node_text(*function, contents).and_then(|value| static_function_atom(&value));
        let arity = static_list_arity(*arguments, contents);
        match (module, function, arity) {
            (Some(module), Some(name), Some(arity)) => RelationshipTarget::Callable {
                module: Some(module),
                name,
                arity,
            },
            _ => dynamic_target(path, node, "apply/3".into(), 3),
        }
    } else {
        dynamic_target(path, node, "apply/3".into(), 3)
    };
    Relationship {
        kind: RelationshipKind::Call,
        target,
        line: node.start_position().row + 1,
        site: node.start_byte(),
    }
}

fn static_function_atom(value: &str) -> Option<String> {
    let value = value.trim();
    value
        .starts_with(':')
        .then(|| normalize_callable_name(value))
        .filter(|value| !value.is_empty())
}

fn static_list_arity(node: Node<'_>, contents: &str) -> Option<usize> {
    if node.kind() != "list" {
        return None;
    }
    let mut arity = 0usize;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "binary_operator"
            && operator_text(child, contents).as_deref() == Some("|")
        {
            return None;
        }
        arity += if child.kind() == "keywords" {
            named_child_count(child)
        } else {
            1
        };
    }
    Some(arity)
}

fn dynamic_target(path: &str, node: Node<'_>, label: String, arity: usize) -> RelationshipTarget {
    RelationshipTarget::Dynamic {
        id: format!(
            "{path}:{}:{}",
            node.start_position().row + 1,
            node.start_byte()
        ),
        label,
        arity,
    }
}

fn named_child_count(node: Node<'_>) -> usize {
    node.named_child_count()
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn is_elixir_special_form(target: &str) -> bool {
    matches!(
        target,
        "alias"
            | "import"
            | "require"
            | "use"
            | "def"
            | "defp"
            | "defmacro"
            | "defmacrop"
            | "defguard"
            | "defguardp"
            | "defdelegate"
            | "defmodule"
            | "defprotocol"
            | "defimpl"
            | "defstruct"
            | "defexception"
            | "case"
            | "cond"
            | "if"
            | "unless"
            | "with"
            | "for"
            | "receive"
            | "try"
            | "fn"
            | "quote"
            | "unquote"
    )
}

fn is_excluded_call_tree(target: &str) -> bool {
    matches!(
        target,
        "alias"
            | "import"
            | "require"
            | "use"
            | "def"
            | "defp"
            | "defmacro"
            | "defmacrop"
            | "defguard"
            | "defguardp"
            | "defdelegate"
            | "defmodule"
            | "defprotocol"
            | "defimpl"
            | "defstruct"
            | "defexception"
            | "quote"
            | "unquote"
    )
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
        let mut relationships = clauses
            .iter()
            .flat_map(|clause| {
                let mut relationships = clause
                    .relationships
                    .iter()
                    .cloned()
                    .map(|relationship| {
                        effective_relationship_arity(relationship, clause.arity, arity)
                    })
                    .collect::<Vec<_>>();
                if arity < clause.arity {
                    relationships.extend(clause.default_relationships.clone());
                }
                relationships
            })
            .collect::<Vec<_>>();
        for head in body
            .default_heads
            .iter()
            .filter(|head| head.kind == kind && head.name == name && arity < head.full_arity)
        {
            relationships.extend(head.default_relationships.clone());
        }
        relationships.sort();
        relationships.dedup();
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
            relationships,
        });
    }
}

fn effective_relationship_arity(
    mut relationship: Relationship,
    source_arity: usize,
    effective_arity: usize,
) -> Relationship {
    if relationship.kind == RelationshipKind::Delegate
        && source_arity != effective_arity
        && let RelationshipTarget::Callable { arity, .. } = &mut relationship.target
    {
        *arity = effective_arity;
    }
    relationship
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
            relationships: Vec::new(),
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
    relationships: Vec<Relationship>,
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
        relationships,
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

fn static_reference_module(text: &str, current_module: &str) -> Option<String> {
    let text = text.trim();
    if text == "__MODULE__" {
        return Some(current_module.to_string());
    }
    if let Some(child) = text.strip_prefix("__MODULE__.") {
        return static_alias(child).then(|| format!("{current_module}.{child}"));
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
    static_alias(text).then(|| text.to_string())
}

fn static_relationship_module(text: &str, current_module: &str) -> Option<String> {
    let text = text.trim();
    if let Some(name) = text
        .strip_prefix(":\"Elixir.")
        .and_then(|name| name.strip_suffix('"'))
        .or_else(|| text.strip_prefix("Elixir."))
    {
        return static_alias(name).then(|| format!("Elixir.{name}"));
    }
    static_reference_module(text, current_module)
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

fn lexical_scope(node: Node<'_>) -> LexicalScope {
    LexicalScope {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
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
        if depths == [0; 4] && value[index..].starts_with(operator) {
            return Some((&value[..index], &value[index + operator.len()..]));
        }
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
    fn extracts_utf8_specification_names_without_panicking() {
        assert_eq!(
            split_top_level_operator("café(term()) when term: term()", "when"),
            Some(("café(term()) ", " term: term()"))
        );

        let file = parse(
            "lib/unicode_contract.ex",
            r#"
defmodule UnicodeContract do
  @spec café(term()) :: :ok
  def café(value), do: value

  @callback résumé(term()) :: :ok
  @macrocallback naïve(term()) :: Macro.t()
end
"#,
        );

        let function = file
            .symbols
            .iter()
            .find(|symbol| symbol.name == "café")
            .expect("UTF-8 function should be extracted");
        assert_eq!(
            function
                .interface_signature
                .as_ref()
                .expect("UTF-8 function should have an interface")
                .specifications
                .len(),
            1
        );
        assert!(file.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Callback
                && symbol.name == "résumé"
                && symbol
                    .interface_signature
                    .as_ref()
                    .is_some_and(|signature| signature.specifications == ["résumé(term()) :: :ok"])
        }));
        assert!(file.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::MacroCallback
                && symbol.name == "naïve"
                && symbol
                    .interface_signature
                    .as_ref()
                    .is_some_and(|signature| {
                        signature.specifications == ["naïve(term()) :: Macro.t()"]
                    })
        }));
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
    fn extracts_lexical_directives_and_distinct_relationships() {
        let file = parse(
            "lib/relationships.ex",
            r#"
defmodule Root.Repo do
  def fetch(value), do: value
  def fetch(value, opts), do: {value, opts}
end

defmodule Root.Helpers do
  def allowed(value), do: value
  def blocked(value), do: value
end

defmodule Root.Macros do
  defmacro build(value), do: value
end

defmodule Root.Behaviour do
  @callback run(term()) :: term()
end

defprotocol Root.Proto do
  def work(value)
end

defmodule My.App do
  alias Root.{Repo, Other}
  import Root.Helpers, only: [allowed: 1], except: [blocked: 1]
  require Root.Macros, as: M
  use Root.Framework
  @behaviour Root.Behaviour

  def run(value \\ fallback()) when guard_check(value) do
    value |> Repo.fetch(opt())
    M.build(value)
    allowed(value)
    blocked(value)
    &Repo.fetch/2
    &local/1
    &(&1 + helper())
    fun.(value)
    fun.(value)
    mod.fetch(value)
    apply(Repo, :fetch, [value])
    apply(mod, name, args)
    quote do
      hidden()
    end
  end

  def local(value), do: value
  defdelegate delegated(value), to: Repo, as: :fetch
end

defimpl Root.Proto, for: My.App do
  def work(value), do: value
end
"#,
        );

        assert_eq!(
            file.imports
                .iter()
                .filter(|directive| directive.kind == DirectiveKind::Alias)
                .map(|directive| (directive.module.as_str(), directive.alias.as_deref()))
                .collect::<Vec<_>>(),
            vec![("Root.Repo", Some("Repo")), ("Root.Other", Some("Other"))]
        );
        assert!(file.imports.iter().any(|directive| {
            directive.kind == DirectiveKind::Import
                && directive.only
                    == Some(DirectiveFilter::Callables(vec![CallableFilter {
                        name: "allowed".into(),
                        arity: 1,
                    }]))
                && directive.except
                    == vec![CallableFilter {
                        name: "blocked".into(),
                        arity: 1,
                    }]
        }));
        assert!(file.imports.iter().any(|directive| {
            directive.kind == DirectiveKind::Require && directive.alias.as_deref() == Some("M")
        }));
        assert!(
            file.imports
                .iter()
                .any(|directive| directive.kind == DirectiveKind::Use)
        );

        let run_zero = file
            .symbols
            .iter()
            .find(|symbol| symbol.id.selector == "module:My.App/fn:run/0")
            .unwrap();
        let run_one = file
            .symbols
            .iter()
            .find(|symbol| symbol.id.selector == "module:My.App/fn:run/1")
            .unwrap();
        assert!(run_zero.relationships.iter().any(|relationship| {
            relationship.kind == RelationshipKind::Call
                && relationship.target
                    == RelationshipTarget::Callable {
                        module: None,
                        name: "fallback".into(),
                        arity: 0,
                    }
        }));
        assert!(!run_one.relationships.iter().any(|relationship| {
            matches!(
                &relationship.target,
                RelationshipTarget::Callable { name, .. } if name == "fallback"
            )
        }));
        for target in [
            RelationshipTarget::Callable {
                module: Some("Repo".into()),
                name: "fetch".into(),
                arity: 2,
            },
            RelationshipTarget::Callable {
                module: Some("M".into()),
                name: "build".into(),
                arity: 1,
            },
            RelationshipTarget::Callable {
                module: None,
                name: "guard_check".into(),
                arity: 1,
            },
            RelationshipTarget::Callable {
                module: None,
                name: "+".into(),
                arity: 2,
            },
        ] {
            assert!(run_one.relationships.iter().any(|relationship| {
                relationship.kind == RelationshipKind::Call && relationship.target == target
            }));
        }
        assert_eq!(
            run_one
                .relationships
                .iter()
                .filter(|relationship| relationship.kind == RelationshipKind::Capture)
                .count(),
            2
        );
        assert!(run_one.relationships.iter().any(|relationship| {
            matches!(relationship.target, RelationshipTarget::Dynamic { .. })
        }));
        let anonymous_sites = run_one
            .relationships
            .iter()
            .filter_map(|relationship| match &relationship.target {
                RelationshipTarget::Dynamic { id, label, .. } if label == "anonymous/1" => Some(id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(anonymous_sites.len(), 2);
        assert!(run_one.relationships.iter().any(|relationship| {
            matches!(
                &relationship.target,
                RelationshipTarget::Dynamic { label, .. } if label == "dynamic.fetch/1"
            )
        }));
        assert!(!run_one.relationships.iter().any(|relationship| {
            matches!(
                &relationship.target,
                RelationshipTarget::Callable { name, .. } if name == "hidden"
            )
        }));

        let delegate = file
            .symbols
            .iter()
            .find(|symbol| symbol.id.selector == "module:My.App/fn:delegated/1")
            .unwrap();
        assert!(delegate.relationships.iter().any(|relationship| {
            relationship.kind == RelationshipKind::Delegate
                && relationship.target
                    == RelationshipTarget::Callable {
                        module: Some("Repo".into()),
                        name: "fetch".into(),
                        arity: 1,
                    }
        }));

        let module = file
            .symbols
            .iter()
            .find(|symbol| symbol.id.selector == "module:My.App")
            .unwrap();
        assert!(module.relationships.iter().any(|relationship| {
            relationship.kind == RelationshipKind::BehaviourImplementation
                && relationship.target
                    == RelationshipTarget::Module {
                        module: "Root.Behaviour".into(),
                        role: ModuleRelationshipRole::Behaviour,
                    }
        }));
        let implementation = file
            .symbols
            .iter()
            .find(|symbol| symbol.id.selector == "impl:Root.Proto/for:My.App")
            .unwrap();
        assert_eq!(
            implementation
                .relationships
                .iter()
                .filter(|relationship| {
                    relationship.kind == RelationshipKind::ProtocolImplementation
                })
                .count(),
            2
        );
    }

    #[test]
    fn expands_all_directive_braces_and_keeps_filter_classes() {
        let file = parse(
            "lib/directives.ex",
            r#"
defmodule Directives do
  import Root.Functions, only: :functions
  import Root.Macros, only: :macros
  import Root.Sigils, only: :sigils
  import Root.{One, Two}
  require Root.{Three, Four}
  use Root.{Five, Six}
end
"#,
        );

        for (module, filter) in [
            ("Root.Functions", DirectiveFilter::Functions),
            ("Root.Macros", DirectiveFilter::Macros),
            ("Root.Sigils", DirectiveFilter::Sigils),
        ] {
            assert!(file.imports.iter().any(|directive| {
                directive.module == module && directive.only == Some(filter.clone())
            }));
        }
        for (kind, module) in [
            (DirectiveKind::Import, "Root.One"),
            (DirectiveKind::Import, "Root.Two"),
            (DirectiveKind::Require, "Root.Three"),
            (DirectiveKind::Require, "Root.Four"),
            (DirectiveKind::Use, "Root.Five"),
            (DirectiveKind::Use, "Root.Six"),
        ] {
            assert!(
                file.imports
                    .iter()
                    .any(|directive| directive.kind == kind && directive.module == module)
            );
        }
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

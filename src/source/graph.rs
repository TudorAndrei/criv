use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use super::paths::read_source_bytes;
use crate::util::write_atomic_in;
use crate::{CrivError, Result};

// Bump when parsed source graph semantics or serialized graph types change.
const GRAPH_CACHE_SCHEMA: &str = "criv.source-graph/2";

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) cache_loads: usize,
    pub(crate) source_reads: usize,
    pub(crate) parsed_files: usize,
    pub(crate) reused_files: usize,
    cache_serializations: usize,
    pub(crate) cache_publications: usize,
    published_bytes: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        cache_loads: 0,
        source_reads: 0,
        parsed_files: 0,
        reused_files: 0,
        cache_serializations: 0,
        cache_publications: 0,
        published_bytes: 0,
    }) };
}

#[cfg(test)]
fn record_work(update: impl FnOnce(&mut WorkCounts)) {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        update(&mut next);
        counts.set(next);
    });
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceGraph {
    pub(crate) files: BTreeMap<String, SourceFile>,
    file_fingerprints: BTreeMap<String, String>,
    #[serde(skip)]
    changed_files: Vec<String>,
    symbol_index: BTreeMap<String, Vec<SymbolId>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceFile {
    pub(crate) path: String,
    pub(crate) language: Language,
    pub(crate) imports: Vec<Import>,
    #[serde(default)]
    modules: Vec<ModuleDecl>,
    pub(crate) symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ModuleDecl {
    name: String,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Import {
    pub(crate) module: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Symbol {
    pub(crate) id: SymbolId,
    pub(crate) name: String,
    pub(crate) kind: SymbolKind,
    pub(crate) parent: Option<String>,
    exported: bool,
    interface_signature: Option<InterfaceSignature>,
    pub(crate) range: SymbolRange,
    pub(crate) calls: Vec<Call>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SymbolRange {
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub(crate) struct SymbolId {
    pub(crate) path: String,
    name: String,
    selector: String,
}

impl SymbolId {
    pub(crate) fn display(&self) -> String {
        format!("{}#{}", self.path, self.selector)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Call {
    pub(crate) target: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum SymbolKind {
    Function,
    Method,
    Class,
}

impl SymbolKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    #[default]
    Unknown,
}

impl Language {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterfaceSignature {
    language: Language,
    symbol_kind: SymbolKind,
    qualified_name: String,
    visibility: Option<String>,
    inputs: Vec<String>,
    output: Option<String>,
    fields: Vec<FieldSignature>,
    variants: Vec<VariantSignature>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub(crate) struct FieldSignature {
    name: String,
    ty: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub(crate) struct VariantSignature {
    name: String,
    fields: Vec<FieldSignature>,
}

#[derive(Deserialize)]
struct GraphCacheFile {
    schema: String,
    graph: SourceGraph,
}

#[derive(Serialize)]
struct BorrowedGraphCacheFile<'a> {
    schema: &'static str,
    graph: &'a SourceGraph,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CacheDisposition {
    Clean,
    Dirty,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceGraphBuild {
    graph: SourceGraph,
    cache: CacheDisposition,
}

impl SourceGraphBuild {
    pub(crate) fn build_incremental(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        let graph = SourceGraph::build_incremental(
            root,
            source_files,
            previous.map(SourceGraphBuild::graph),
        )?;
        let cache = if previous.is_some_and(|previous| {
            previous.cache == CacheDisposition::Clean
                && graph.changed_files().is_empty()
                && graph_cache_path(root).is_file()
        }) {
            CacheDisposition::Clean
        } else {
            CacheDisposition::Dirty
        };
        Ok(Self { graph, cache })
    }

    pub(crate) fn disabled() -> Self {
        Self {
            graph: SourceGraph::default(),
            cache: CacheDisposition::Clean,
        }
    }

    pub(crate) fn graph(&self) -> &SourceGraph {
        &self.graph
    }

    pub(crate) fn paths(&self) -> Vec<String> {
        self.graph.files.keys().cloned().collect()
    }

    pub(crate) fn publish(mut self, root: &Path) -> Result<Self> {
        if self.cache == CacheDisposition::Dirty {
            store_cached(root, &self.graph)?;
            self.cache = CacheDisposition::Clean;
        }
        Ok(self)
    }
}

pub(crate) fn load_cached(root: &Path) -> Option<SourceGraphBuild> {
    #[cfg(test)]
    record_work(|counts| counts.cache_loads += 1);

    let path = graph_cache_path(root);
    let contents = fs::read_to_string(path).ok()?;
    let cache = serde_json::from_str::<GraphCacheFile>(&contents).ok()?;
    (cache.schema == GRAPH_CACHE_SCHEMA).then_some(SourceGraphBuild {
        graph: cache.graph,
        cache: CacheDisposition::Clean,
    })
}

fn store_cached(root: &Path, graph: &SourceGraph) -> Result<()> {
    let cache = BorrowedGraphCacheFile {
        schema: GRAPH_CACHE_SCHEMA,
        graph,
    };
    #[cfg(test)]
    record_work(|counts| counts.cache_serializations += 1);
    let contents = serde_json::to_string_pretty(&cache)
        .map_err(|err| CrivError::new(format!("failed to serialize source graph cache: {err}")))?;
    let contents = format!("{contents}\n");
    write_atomic_in(
        root,
        Path::new(".criv"),
        Path::new(".criv/source-graph.json"),
        &contents,
    )?;
    #[cfg(test)]
    record_work(|counts| {
        counts.cache_publications += 1;
        counts.published_bytes += contents.len();
    });
    Ok(())
}

fn graph_cache_path(root: &Path) -> std::path::PathBuf {
    root.join(".criv/source-graph.json")
}

impl InterfaceSignature {
    fn hash(&self) -> String {
        blake3::hash(self.stable_text().as_bytes())
            .to_hex()
            .to_string()
    }

    fn from_source(
        language: Language,
        symbol_kind: SymbolKind,
        name: &str,
        parent: Option<&str>,
        exported: bool,
        source: &str,
    ) -> Self {
        let qualified_name = parent
            .map(|parent| format!("{parent}.{name}"))
            .unwrap_or_else(|| name.to_string());
        let visibility = exported.then(|| visibility_text(language, source));
        let (inputs, output) = match symbol_kind {
            SymbolKind::Function | SymbolKind::Method => function_signature(language, source),
            SymbolKind::Class => (Vec::new(), None),
        };
        let (fields, variants) = match symbol_kind {
            SymbolKind::Class => aggregate_signature(language, source),
            SymbolKind::Function | SymbolKind::Method => (Vec::new(), Vec::new()),
        };

        Self {
            language,
            symbol_kind,
            qualified_name,
            visibility,
            inputs,
            output,
            fields,
            variants,
        }
    }

    fn stable_text(&self) -> String {
        let mut fields = self.fields.clone();
        fields.sort();
        let mut variants = self.variants.clone();
        variants.sort();
        let field_text = fields
            .iter()
            .map(|field| format!("{}:{}", field.name, field.ty.as_deref().unwrap_or("")))
            .collect::<Vec<_>>()
            .join(",");
        let variant_text = variants
            .iter()
            .map(|variant| {
                let fields = variant
                    .fields
                    .iter()
                    .map(|field| format!("{}:{}", field.name, field.ty.as_deref().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}({fields})", variant.name)
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "language={}\nkind={}\nname={}\nvisibility={}\ninputs={}\noutput={}\nfields={field_text}\nvariants={variant_text}\n",
            self.language.as_str(),
            self.symbol_kind.as_str(),
            self.qualified_name,
            self.visibility.as_deref().unwrap_or(""),
            self.inputs.join(","),
            self.output.as_deref().unwrap_or(""),
        )
    }
}

impl SourceGraph {
    fn build_incremental(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        let mut graph = Self::default();
        for source_file in source_files {
            #[cfg(test)]
            record_work(|counts| counts.source_reads += 1);
            let bytes = read_source_bytes(root, source_file).map_err(|error| {
                CrivError::new(format!(
                    "failed to read selected file `{source_file}`: {error}"
                ))
            })?;
            if !content_inspector::inspect(&bytes).is_text() {
                continue;
            }
            let fingerprint = blake3::hash(&bytes).to_hex().to_string();
            let contents = String::from_utf8_lossy(&bytes);
            let reused = previous
                .filter(|previous| {
                    previous.file_fingerprints.get(source_file) == Some(&fingerprint)
                })
                .and_then(|previous| previous.files.get(source_file).cloned());
            let parsed = if let Some(parsed) = reused {
                #[cfg(test)]
                record_work(|counts| counts.reused_files += 1);
                parsed
            } else {
                #[cfg(test)]
                record_work(|counts| counts.parsed_files += 1);
                graph.changed_files.push(source_file.clone());
                parse_source_file(source_file, &contents)
            };
            for symbol in &parsed.symbols {
                graph
                    .symbol_index
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(symbol.id.clone());
                graph
                    .symbol_index
                    .entry(symbol.id.selector.clone())
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
        let (path, selector) = query.split_once('#').unwrap_or(("", query));
        if !path.is_empty() {
            return self
                .files
                .get(path)
                .and_then(|file| {
                    file.symbols
                        .iter()
                        .find(|symbol| symbol.id.selector == selector || symbol.name == selector)
                })
                .map(|symbol| symbol.id.clone());
        }

        self.symbol_index
            .get(selector)
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

    #[cfg(test)]
    pub(crate) fn without_changed_files(&self) -> Self {
        let mut graph = self.clone();
        graph.changed_files.clear();
        graph
    }

    pub(crate) fn resolve_call(&self, caller: &SymbolId, target: &str) -> Option<SymbolId> {
        self.files
            .get(&caller.path)
            .and_then(|file| file.symbols.iter().find(|symbol| symbol.name == target))
            .map(|symbol| symbol.id.clone())
            .or_else(|| self.resolve_symbol(target))
    }

    pub(crate) fn interface_hash(&self, query: &str) -> Option<String> {
        let symbol_id = self.resolve_symbol(query)?;
        self.symbol(&symbol_id)
            .and_then(|symbol| symbol.interface_signature.as_ref())
            .map(InterfaceSignature::hash)
    }

    pub(crate) fn canonical_symbol_target(&self, query: &str) -> Option<String> {
        self.resolve_symbol(query).map(|symbol| symbol.display())
    }

    fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.files
            .get(&id.path)
            .and_then(|file| file.symbols.iter().find(|symbol| &symbol.id == id))
    }
}

fn parse_source_file(path: &str, contents: &str) -> SourceFile {
    parse_tree_sitter_file(path, contents)
        .unwrap_or_else(|| parse_source_file_fallback(path, contents))
}

fn symbol_selector(kind: SymbolKind, parent: Option<&str>, name: &str) -> String {
    match (kind, parent) {
        (SymbolKind::Function, _) => format!("fn:{name}"),
        (SymbolKind::Method, Some(parent)) => format!("type:{parent}/member:{name}"),
        (SymbolKind::Method, None) => format!("member:{name}"),
        (SymbolKind::Class, _) => format!("type:{name}"),
    }
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
        modules: Vec::new(),
        symbols: Vec::new(),
    };
    collect_tree_sitter_nodes(
        tree.root_node(),
        contents,
        path,
        language,
        None,
        None,
        &mut file,
    );
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
    module_parent: Option<String>,
    file: &mut SourceFile,
) {
    for import in tree_sitter_imports(node, contents, language) {
        file.imports.push(Import {
            module: import,
            line: node.start_position().row + 1,
        });
    }
    let child_module_parent = if let Some(name) = tree_sitter_module(node, contents, language) {
        let name = module_parent
            .as_deref()
            .map_or_else(|| name.clone(), |parent| format!("{parent}::{name}"));
        file.modules.push(ModuleDecl {
            name: name.clone(),
            line: node.start_position().row + 1,
        });
        Some(name)
    } else {
        module_parent.clone()
    };

    if let Some(symbol) = tree_sitter_symbol(node, contents, path, language, parent.as_deref()) {
        let symbol_parent = if symbol.kind == SymbolKind::Class {
            Some(symbol.name.clone())
        } else {
            parent.clone()
        };
        file.symbols.push(symbol);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_tree_sitter_nodes(
                child,
                contents,
                path,
                language,
                symbol_parent.clone(),
                child_module_parent.clone(),
                file,
            );
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
        collect_tree_sitter_nodes(
            child,
            contents,
            path,
            language,
            impl_parent.clone(),
            child_module_parent.clone(),
            file,
        );
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
                .map(|text| clean_js_module(&text))
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

    let source = node_text(node, contents).unwrap_or_default();
    let selector = symbol_selector(kind, parent, &name);
    let interface_signature = Some(InterfaceSignature::from_source(
        language,
        kind,
        &name,
        parent,
        tree_sitter_exported(node, contents, language),
        &source,
    ));

    Some(Symbol {
        id: SymbolId {
            path: path.into(),
            name: name.clone(),
            selector,
        },
        name,
        kind,
        parent: parent.map(str::to_string),
        exported: tree_sitter_exported(node, contents, language),
        interface_signature,
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
        Language::Unknown => false,
    }
}

fn visibility_text(language: Language, source: &str) -> String {
    let trimmed = source.trim_start();
    match language {
        Language::Rust if trimmed.starts_with("pub(") => trimmed
            .split_whitespace()
            .next()
            .unwrap_or("pub")
            .trim_end_matches('{')
            .to_string(),
        Language::Rust => "pub".into(),
        Language::TypeScript | Language::JavaScript => "export".into(),
        Language::Python | Language::Go | Language::Unknown => "public".into(),
    }
}

fn function_signature(language: Language, source: &str) -> (Vec<String>, Option<String>) {
    match language {
        Language::Rust => rust_function_signature(source),
        Language::TypeScript | Language::JavaScript => typescript_function_signature(source),
        _ => generic_function_signature(source),
    }
}

fn rust_function_signature(source: &str) -> (Vec<String>, Option<String>) {
    let header = source
        .split_once('{')
        .map(|(head, _)| head)
        .unwrap_or(source)
        .trim();
    let inputs = paren_contents(header)
        .map(split_signature_list)
        .unwrap_or_default();
    let output = header
        .rsplit_once(')')
        .map(|(_, tail)| tail.trim())
        .and_then(|tail| tail.strip_prefix("->"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_signature_text);
    (inputs, output)
}

fn typescript_function_signature(source: &str) -> (Vec<String>, Option<String>) {
    let header = source
        .split_once("=>")
        .map(|(head, _)| head)
        .or_else(|| source.split_once('{').map(|(head, _)| head))
        .unwrap_or(source)
        .trim();
    let inputs = paren_contents(header)
        .map(split_signature_list)
        .unwrap_or_default();
    let output = header
        .rsplit_once(')')
        .map(|(_, tail)| tail.trim())
        .and_then(|tail| tail.strip_prefix(':'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_signature_text);
    (inputs, output)
}

fn generic_function_signature(source: &str) -> (Vec<String>, Option<String>) {
    let header = source
        .split_once('{')
        .map(|(head, _)| head)
        .unwrap_or(source)
        .trim();
    let inputs = paren_contents(header)
        .map(split_signature_list)
        .unwrap_or_default();
    (inputs, None)
}

fn aggregate_signature(
    language: Language,
    source: &str,
) -> (Vec<FieldSignature>, Vec<VariantSignature>) {
    match language {
        Language::Rust if source.trim_start().starts_with("enum ") || source.contains(" enum ") => {
            (Vec::new(), rust_enum_variants(source))
        }
        Language::Rust => (rust_fields(source), Vec::new()),
        Language::TypeScript | Language::JavaScript => (typescript_members(source), Vec::new()),
        _ => (Vec::new(), Vec::new()),
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
    (end > start).then_some(&source[start + 1..end])
}

fn brace_contents(source: &str) -> Option<&str> {
    let start = source.find('{')?;
    let end = source.rfind('}')?;
    (end > start).then_some(&source[start + 1..end])
}

fn split_signature_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(normalize_signature_text)
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_signature_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
            file.imports.push(Import {
                module: import,
                line: line_no,
            });
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
            let id = SymbolId {
                path: path.into(),
                name: name.clone(),
                selector: symbol_selector(kind, parent.as_deref(), &name),
            };
            file.symbols.push(Symbol {
                id,
                interface_signature: Some(InterfaceSignature::from_source(
                    language,
                    kind,
                    &name,
                    parent.as_deref(),
                    is_exported_symbol(trimmed, language),
                    trimmed,
                )),
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

fn fallback_module_decl(line: &str, language: Language) -> Option<String> {
    let prefix = match language {
        Language::Rust => "mod ",
        Language::TypeScript | Language::JavaScript => line
            .strip_prefix("export ")
            .map(|_| "namespace ")
            .unwrap_or("namespace "),
        Language::Go => "package ",
        _ => return None,
    };
    let rest = if matches!(language, Language::TypeScript | Language::JavaScript) {
        line.strip_prefix("export ")
            .unwrap_or(line)
            .strip_prefix(prefix)?
    } else {
        line.strip_prefix(prefix)?
    };
    let name = rest
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .next()?
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn parse_import(line: &str, language: Language) -> Option<String> {
    parse_imports(line, language).into_iter().next()
}

fn parse_imports(line: &str, language: Language) -> Vec<String> {
    match language {
        Language::Rust => line
            .strip_prefix("use ")
            .or_else(|| line.strip_prefix("pub use "))
            .map(parse_rust_import_body)
            .or_else(|| {
                line.strip_prefix("mod ")
                    .map(|value| vec![value.trim_end_matches(';').trim().to_string()])
            })
            .unwrap_or_default(),
        Language::TypeScript | Language::JavaScript => {
            if let Some((_, module)) = line.split_once(" from ") {
                vec![clean_js_module(module)]
            } else {
                line.strip_prefix("import ")
                    .map(clean_js_module)
                    .into_iter()
                    .collect()
            }
        }
        Language::Python => line
            .strip_prefix("import ")
            .map(|value| value.split_whitespace().next().unwrap_or(value).to_string())
            .or_else(|| {
                line.strip_prefix("from ")
                    .and_then(|value| value.split_once(" import "))
                    .map(|(module, _)| module.to_string())
            })
            .into_iter()
            .collect(),
        Language::Go => line
            .strip_prefix("import ")
            .map(|value| {
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('(')
                    .trim_matches(')')
                    .to_string()
            })
            .into_iter()
            .collect(),
        Language::Unknown => Vec::new(),
    }
}

fn parse_rust_imports(line: &str) -> Vec<String> {
    line.trim()
        .strip_prefix("use ")
        .or_else(|| line.trim().strip_prefix("pub use "))
        .map(parse_rust_import_body)
        .unwrap_or_default()
}

fn parse_rust_import_body(body: &str) -> Vec<String> {
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

    if let Some((open, close)) = rust_group_bounds(body) {
        let head = body[..open].trim().trim_end_matches("::");
        let base = join_rust_import(prefix, head);
        return split_top_level_commas(&body[open + 1..close])
            .into_iter()
            .flat_map(|part| expand_rust_import(&base, part))
            .collect();
    }

    let leaf = body
        .split_once(" as ")
        .map(|(module, _)| module)
        .unwrap_or(body)
        .trim();
    let module = join_rust_import(prefix, leaf);
    if module.is_empty() {
        Vec::new()
    } else {
        vec![module]
    }
}

fn rust_group_bounds(value: &str) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut open = None;
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    open = Some(index);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return open.map(|open| (open, index));
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                rows.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    rows.push(value[start..].trim());
    rows.into_iter().filter(|row| !row.is_empty()).collect()
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
            } else {
                after_keyword(line, "class ").map(|name| (name, SymbolKind::Class))
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
    use std::fs;
    use tempfile::TempDir;

    #[cfg(unix)]
    #[test]
    fn incremental_build_rejects_a_linked_source_file() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path().join("vault");
        let outside = temp.path().join("outside");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.rs"), "pub fn secret() {}\n").unwrap();
        symlink(outside.join("secret.rs"), root.join("src/secret.rs")).unwrap();

        let error = SourceGraph::build_incremental(&root, &["src/secret.rs".into()], None)
            .expect_err("source graph should reject a linked source file");

        assert!(error.to_string().contains("linked source path"));
    }

    #[test]
    fn source_graph_cache_round_trips_and_skips_changed_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        let mut build =
            SourceGraphBuild::build_incremental(root, &["src/lib.rs".into()], None).unwrap();
        assert_eq!(build.graph.changed_files(), &["src/lib.rs".to_string()]);
        build.graph.changed_files.push("scratch.rs".to_string());

        let build = build.publish(root).unwrap();
        let loaded = load_cached(root).unwrap();

        let mut expected = build.graph.clone();
        expected.changed_files.clear();
        assert_eq!(loaded.graph(), &expected);
        assert!(loaded.graph().changed_files().is_empty());
    }

    #[test]
    fn source_graph_cache_is_not_republished_when_unchanged() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        let first = SourceGraphBuild::build_incremental(root, &["src/lib.rs".into()], None)
            .unwrap()
            .publish(root)
            .unwrap();
        let path = graph_cache_path(root);
        let before = fs::read_to_string(&path).unwrap();
        let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_234_567);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        reset_work_counts();
        SourceGraphBuild::build_incremental(root, &["src/lib.rs".into()], Some(&first))
            .unwrap()
            .publish(root)
            .unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(work_counts().cache_serializations, 0);
        assert_eq!(work_counts().cache_publications, 0);
    }

    #[test]
    fn work_counts_distinguish_parse_reuse_and_cache_publication() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        reset_work_counts();

        let first = SourceGraphBuild::build_incremental(root, &["src/lib.rs".into()], None)
            .unwrap()
            .publish(root)
            .unwrap();
        SourceGraphBuild::build_incremental(root, &["src/lib.rs".into()], Some(&first))
            .unwrap()
            .publish(root)
            .unwrap();

        let counts = work_counts();
        assert_eq!(counts.source_reads, 2);
        assert_eq!(counts.parsed_files, 1);
        assert_eq!(counts.reused_files, 1);
        assert_eq!(counts.cache_serializations, 1);
        assert_eq!(counts.cache_publications, 1);
        assert!(counts.published_bytes > 0);
    }

    #[test]
    fn every_dirty_or_untrusted_graph_cache_publishes_exactly_once() {
        fn baseline() -> (TempDir, SourceGraphBuild) {
            let temp = TempDir::new().unwrap();
            fs::create_dir_all(temp.path().join("src")).unwrap();
            fs::write(temp.path().join("src/lib.rs"), "pub fn run() {}\n").unwrap();
            let build =
                SourceGraphBuild::build_incremental(temp.path(), &["src/lib.rs".into()], None)
                    .unwrap()
                    .publish(temp.path())
                    .unwrap();
            (temp, build)
        }

        fn publish_and_assert(
            root: &Path,
            source_files: &[String],
            previous: Option<&SourceGraphBuild>,
        ) -> SourceGraphBuild {
            reset_work_counts();
            let build = SourceGraphBuild::build_incremental(root, source_files, previous)
                .unwrap()
                .publish(root)
                .unwrap();
            let counts = work_counts();
            assert_eq!(counts.cache_serializations, 1);
            assert_eq!(counts.cache_publications, 1);
            build
        }

        let (missing, previous) = baseline();
        fs::remove_file(graph_cache_path(missing.path())).unwrap();
        publish_and_assert(missing.path(), &["src/lib.rs".into()], Some(&previous));

        let (garbage, _) = baseline();
        fs::write(graph_cache_path(garbage.path()), "garbage\n").unwrap();
        let loaded = load_cached(garbage.path());
        assert!(loaded.is_none());
        publish_and_assert(garbage.path(), &["src/lib.rs".into()], loaded.as_ref());

        let (wrong_schema, _) = baseline();
        let cache_path = graph_cache_path(wrong_schema.path());
        let cache = fs::read_to_string(&cache_path).unwrap().replacen(
            GRAPH_CACHE_SCHEMA,
            "criv.source-graph/wrong",
            1,
        );
        fs::write(&cache_path, cache).unwrap();
        let loaded = load_cached(wrong_schema.path());
        assert!(loaded.is_none());
        publish_and_assert(wrong_schema.path(), &["src/lib.rs".into()], loaded.as_ref());

        let (edited, previous) = baseline();
        fs::write(edited.path().join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
        publish_and_assert(edited.path(), &["src/lib.rs".into()], Some(&previous));

        let (same_size, previous) = baseline();
        let source = same_size.path().join("src/lib.rs");
        let modified = fs::metadata(&source).unwrap().modified().unwrap();
        fs::write(&source, "pub fn two() {}\n").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        publish_and_assert(same_size.path(), &["src/lib.rs".into()], Some(&previous));

        let (added, previous) = baseline();
        fs::write(added.path().join("src/new.rs"), "pub fn new() {}\n").unwrap();
        publish_and_assert(
            added.path(),
            &["src/lib.rs".into(), "src/new.rs".into()],
            Some(&previous),
        );

        let (renamed, previous) = baseline();
        fs::rename(
            renamed.path().join("src/lib.rs"),
            renamed.path().join("src/main.rs"),
        )
        .unwrap();
        publish_and_assert(renamed.path(), &["src/main.rs".into()], Some(&previous));

        let (deleted, previous) = baseline();
        fs::remove_file(deleted.path().join("src/lib.rs")).unwrap();
        publish_and_assert(deleted.path(), &[], Some(&previous));
    }

    #[cfg(unix)]
    #[test]
    fn graph_cache_publication_rejects_a_symlinked_criv_directory() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path().join("vault");
        let outside = temp.path().join("outside");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        symlink(&outside, root.join(".criv")).unwrap();
        let build =
            SourceGraphBuild::build_incremental(&root, &["src/lib.rs".into()], None).unwrap();

        let error = build.publish(&root).unwrap_err();

        assert!(error.to_string().contains("symlinked vault path component"));
        assert!(!outside.join("source-graph.json").exists());
    }

    #[test]
    fn incremental_graph_reparses_same_size_content_with_restored_mtime() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let path = root.join("src/lib.rs");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "pub fn one() {}\n").unwrap();
        let before = SourceGraph::build_incremental(root, &["src/lib.rs".into()], None).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();

        fs::write(&path, "pub fn two() {}\n").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let after =
            SourceGraph::build_incremental(root, &["src/lib.rs".into()], Some(&before)).unwrap();

        assert!(after.resolve_symbol("two").is_some());
        assert!(after.resolve_symbol("one").is_none());
        assert_eq!(after.changed_files(), &["src/lib.rs".to_string()]);
    }

    #[test]
    fn source_graph_cache_ignores_garbage_and_wrong_schema() {
        let temp = TempDir::new().unwrap();
        let cache_path = graph_cache_path(temp.path());
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();

        fs::write(&cache_path, "garbage\n").unwrap();
        assert!(load_cached(temp.path()).is_none());

        fs::write(
            &cache_path,
            r#"{"schema":"criv.source-graph/0","graph":{}}"#,
        )
        .unwrap();
        assert!(load_cached(temp.path()).is_none());
    }

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
    fn normalizes_grouped_and_aliased_rust_imports() {
        let file = parse_source_file(
            "src/lib.rs",
            r#"
use crate::{infra::db, ui::{self, view as ui_view}};
use crate::infra as infra;
"#,
        );
        let imports = file
            .imports
            .iter()
            .map(|import| import.module.as_str())
            .collect::<Vec<_>>();

        assert!(imports.contains(&"crate::infra"));
        assert!(imports.contains(&"crate::infra::db"));
        assert!(imports.contains(&"crate::ui"));
        assert!(imports.contains(&"crate::ui::view"));
    }

    #[test]
    fn rust_interface_hash_ignores_function_body_changes() {
        let before = parse_source_file(
            "src/lib.rs",
            r#"
pub fn run(input: String) -> usize {
  input.len()
}
"#,
        );
        let after = parse_source_file(
            "src/lib.rs",
            r#"
pub fn run(input: String) -> usize {
  println!("{}", input);
  42
}
"#,
        );

        assert_eq!(
            before.symbols[0]
                .interface_signature
                .as_ref()
                .unwrap()
                .hash(),
            after.symbols[0]
                .interface_signature
                .as_ref()
                .unwrap()
                .hash()
        );
    }

    #[test]
    fn rust_interface_hash_changes_when_function_signature_changes() {
        let before = parse_source_file("src/lib.rs", "pub fn run(input: String) -> usize { 1 }\n");
        let after = parse_source_file(
            "src/lib.rs",
            "pub fn run(input: String, verbose: bool) -> usize { 1 }\n",
        );

        assert_ne!(
            before.symbols[0]
                .interface_signature
                .as_ref()
                .unwrap()
                .hash(),
            after.symbols[0]
                .interface_signature
                .as_ref()
                .unwrap()
                .hash()
        );
    }

    #[test]
    fn rust_interface_hash_tracks_struct_fields_and_enum_variants() {
        let before = parse_source_file(
            "src/lib.rs",
            r#"
pub struct Config {
  pub root: String,
}
pub enum Mode {
  Check,
  Watch,
}
"#,
        );
        let after = parse_source_file(
            "src/lib.rs",
            r#"
pub struct Config {
  pub root: String,
  pub verbose: bool,
}
pub enum Mode {
  Check,
  Watch,
  Serve,
}
"#,
        );

        let before_config = before
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Config")
            .unwrap();
        let after_config = after
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Config")
            .unwrap();
        let before_mode = before
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Mode")
            .unwrap();
        let after_mode = after
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Mode")
            .unwrap();

        assert_ne!(
            before_config.interface_signature.as_ref().unwrap().hash(),
            after_config.interface_signature.as_ref().unwrap().hash()
        );
        assert_ne!(
            before_mode.interface_signature.as_ref().unwrap().hash(),
            after_mode.interface_signature.as_ref().unwrap().hash()
        );
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
            graph
                .symbol_index
                .entry(symbol.id.selector.clone())
                .or_default()
                .push(symbol.id.clone());
        }
        graph.files.insert(file.path.clone(), file);

        let rows = graph.attack_surface();
        assert!(rows.contains(&"src/lib.rs#fn:api".to_string()));
        assert!(rows.contains(&"src/lib.rs#fn:caller".to_string()));
        assert!(!rows.contains(&"src/lib.rs#fn:helper".to_string()));
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
                graph
                    .symbol_index
                    .entry(symbol.id.selector.clone())
                    .or_default()
                    .push(symbol.id.clone());
            }
            graph.files.insert(file.path.clone(), file);
        }

        assert_eq!(
            graph.callees("src/local.rs#caller"),
            vec!["src/local.rs#fn:target"]
        );
    }

    #[test]
    fn semantic_selectors_disambiguate_same_file_methods() {
        let file = parse_source_file(
            "src/views.ts",
            r#"
class A {
  render() {}
}
class B {
  render() {}
}
"#,
        );

        let selectors = file
            .symbols
            .iter()
            .map(|symbol| symbol.id.display())
            .collect::<Vec<_>>();

        assert!(selectors.contains(&"src/views.ts#type:A/member:render".to_string()));
        assert!(selectors.contains(&"src/views.ts#type:B/member:render".to_string()));
    }

    #[test]
    fn legacy_symbol_names_still_resolve() {
        let file = parse_source_file("src/lib.rs", "fn run() {}\n");
        let mut graph = SourceGraph::default();
        for symbol in &file.symbols {
            graph
                .symbol_index
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol.id.clone());
            graph
                .symbol_index
                .entry(symbol.id.selector.clone())
                .or_default()
                .push(symbol.id.clone());
        }
        graph.files.insert(file.path.clone(), file);

        assert_eq!(
            graph.resolve_symbol("src/lib.rs#run").unwrap().display(),
            "src/lib.rs#fn:run"
        );
        assert_eq!(
            graph.resolve_symbol("src/lib.rs#fn:run").unwrap().display(),
            "src/lib.rs#fn:run"
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
    fn typescript_interface_hash_tracks_function_and_class_shapes() {
        let before = parse_source_file(
            "src/app.ts",
            r#"
export function run(input: string): number {
  return input.length;
}
export class Service {
  run(input: string): number {
    return input.length;
  }
}
"#,
        );
        let body_only = parse_source_file(
            "src/app.ts",
            r#"
export function run(input: string): number {
  console.log(input);
  return 1;
}
export class Service {
  run(input: string): number {
    console.log(input);
    return 1;
  }
}
"#,
        );
        let changed = parse_source_file(
            "src/app.ts",
            r#"
export function run(input: string, fallback: number): number {
  return fallback;
}
export class Service {
  run(input: string): string {
    return input;
  }
}
"#,
        );

        let before_run = before
            .symbols
            .iter()
            .find(|symbol| symbol.name == "run" && symbol.kind == SymbolKind::Function)
            .unwrap();
        let body_run = body_only
            .symbols
            .iter()
            .find(|symbol| symbol.name == "run" && symbol.kind == SymbolKind::Function)
            .unwrap();
        let changed_run = changed
            .symbols
            .iter()
            .find(|symbol| symbol.name == "run" && symbol.kind == SymbolKind::Function)
            .unwrap();

        assert_eq!(
            before_run.interface_signature.as_ref().unwrap().hash(),
            body_run.interface_signature.as_ref().unwrap().hash()
        );
        assert_ne!(
            before_run.interface_signature.as_ref().unwrap().hash(),
            changed_run.interface_signature.as_ref().unwrap().hash()
        );
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

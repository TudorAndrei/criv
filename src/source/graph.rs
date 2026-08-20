use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::thread;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use criv_state_wire::source_identity::{SourceIdentity, SourceSelector};

use super::paths::read_source_bytes;
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

mod elixir;

// Bump when parsed source graph semantics or serialized graph types change.
const GRAPH_CACHE_SCHEMA: &str = "criv.source-graph/3";
const MAX_SOURCE_WORKERS: usize = 16;
const MIN_FILES_PER_SOURCE_WORKER: usize = 1_024;

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
    #[serde(skip)]
    elixir_relationships: elixir::ElixirRelationships,
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
    #[serde(default)]
    pub(crate) site: usize,
    #[serde(default)]
    pub(crate) kind: DirectiveKind,
    #[serde(default)]
    pub(crate) owner: Option<SymbolOwner>,
    #[serde(default)]
    pub(crate) scope: Option<LexicalScope>,
    #[serde(default)]
    pub(crate) alias: Option<String>,
    #[serde(default)]
    pub(crate) only: Option<DirectiveFilter>,
    #[serde(default)]
    pub(crate) except: Vec<CallableFilter>,
    #[serde(default)]
    pub(crate) absolute: bool,
}

impl Import {
    fn legacy(module: String, line: usize) -> Self {
        Self {
            module,
            line,
            site: 0,
            kind: DirectiveKind::Legacy,
            owner: None,
            scope: None,
            alias: None,
            only: None,
            except: Vec::new(),
            absolute: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) struct LexicalScope {
    start_byte: usize,
    end_byte: usize,
}

#[derive(Debug, Default, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum DirectiveKind {
    Alias,
    Import,
    Require,
    Use,
    #[default]
    Legacy,
}

impl DirectiveKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Alias => "alias",
            Self::Import | Self::Legacy => "import",
            Self::Require => "require",
            Self::Use => "use",
        }
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum DirectiveFilter {
    Callables(Vec<CallableFilter>),
    Functions,
    Macros,
    Sigils,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) struct CallableFilter {
    name: String,
    arity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Symbol {
    pub(crate) id: SymbolId,
    pub(crate) name: String,
    pub(crate) kind: SymbolKind,
    pub(crate) parent: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<SymbolOwner>,
    #[serde(default)]
    pub(crate) arity: Option<usize>,
    exported: bool,
    interface_signature: Option<InterfaceSignature>,
    pub(crate) range: SymbolRange,
    #[serde(default)]
    clause_ranges: Vec<SymbolRange>,
    pub(crate) calls: Vec<Call>,
    #[serde(default)]
    pub(crate) relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SymbolRange {
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub(crate) enum SymbolOwner {
    Module { name: String },
    Implementation { protocol: String, for_type: String },
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub(crate) struct SymbolId {
    pub(crate) path: String,
    name: String,
    selector: String,
}

impl SymbolId {
    pub(crate) fn display(&self) -> String {
        SourceIdentity::symbol(
            self.path.clone(),
            SourceSelector::opaque(self.selector.clone()),
        )
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Call {
    pub(crate) target: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum RelationshipKind {
    Call,
    Capture,
    Delegate,
    ProtocolImplementation,
    BehaviourImplementation,
}

impl RelationshipKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Call => "calls",
            Self::Capture => "captures",
            Self::Delegate => "delegates",
            Self::ProtocolImplementation => "protocol-implementation",
            Self::BehaviourImplementation => "behaviour-implementation",
        }
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) struct Relationship {
    pub(crate) kind: RelationshipKind,
    pub(crate) target: RelationshipTarget,
    pub(crate) line: usize,
    #[serde(default)]
    pub(crate) site: usize,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum ModuleRelationshipRole {
    Protocol,
    ForType,
    Behaviour,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum RelationshipTarget {
    Callable {
        module: Option<String>,
        name: String,
        arity: usize,
    },
    Module {
        module: String,
        role: ModuleRelationshipRole,
    },
    Dynamic {
        id: String,
        label: String,
        arity: usize,
    },
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum SymbolKind {
    Function,
    Method,
    Class,
    Module,
    Protocol,
    Implementation,
    Struct,
    Exception,
    Behaviour,
    Macro,
    Guard,
    Callback,
    MacroCallback,
}

impl SymbolKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Module => "module",
            Self::Protocol => "protocol",
            Self::Implementation => "implementation",
            Self::Struct => "struct",
            Self::Exception => "exception",
            Self::Behaviour => "behaviour",
            Self::Macro => "macro",
            Self::Guard => "guard",
            Self::Callback => "callback",
            Self::MacroCallback => "macro-callback",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum SymbolResolution {
    Resolved(SymbolId),
    Ambiguous(Vec<SymbolId>),
    Missing,
}

#[derive(Debug, Default, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Elixir,
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
            Self::Elixir => "elixir",
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
    #[serde(default)]
    arity: Option<usize>,
    #[serde(default)]
    guards: Vec<String>,
    #[serde(default)]
    defaults: Vec<String>,
    #[serde(default)]
    specifications: Vec<String>,
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
pub(super) struct SourceGraphBuild {
    graph: SourceGraph,
    cache: CacheDisposition,
}

impl SourceGraphBuild {
    #[cfg(test)]
    pub(super) fn build_incremental(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::build_incremental_from(&files, source_files, previous)
    }

    pub(super) fn build_incremental_from(
        files: &RepositoryFiles,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        Self::build_incremental_with_workers_inner(
            files,
            source_files,
            previous,
            source_worker_count(source_files.len()),
        )
    }

    #[cfg(test)]
    pub(crate) fn build_incremental_with_workers(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
        workers: usize,
    ) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        assert!(workers > 0, "Source test worker count must be positive");
        Self::build_incremental_with_workers_inner(&files, source_files, previous, workers)
    }

    fn build_incremental_with_workers_inner(
        files: &RepositoryFiles,
        source_files: &[String],
        previous: Option<&Self>,
        workers: usize,
    ) -> Result<Self> {
        let graph = SourceGraph::build_incremental_with_workers(
            files,
            source_files,
            previous.map(SourceGraphBuild::graph),
            workers,
        )?;
        let can_reuse = previous.is_some_and(|previous| {
            previous.cache == CacheDisposition::Clean && graph.changed_files().is_empty()
        });
        let cache = if can_reuse && files.file_exists(Path::new(".criv/source-graph.json"))? {
            CacheDisposition::Clean
        } else {
            CacheDisposition::Dirty
        };
        Ok(Self { graph, cache })
    }

    pub(super) fn disabled() -> Self {
        Self {
            graph: SourceGraph::default(),
            cache: CacheDisposition::Clean,
        }
    }

    pub(super) fn reused(&self) -> Self {
        let mut reused = self.clone();
        reused.graph.changed_files.clear();
        reused
    }

    pub(super) fn graph(&self) -> &SourceGraph {
        &self.graph
    }

    pub(super) fn paths(&self) -> Vec<String> {
        self.graph.files.keys().cloned().collect()
    }

    #[cfg(test)]
    pub(super) fn publish(self, root: &Path) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        self.publish_from(&files)
    }

    pub(super) fn publish_from(mut self, files: &RepositoryFiles) -> Result<Self> {
        if self.cache == CacheDisposition::Dirty {
            store_cached(files, &self.graph)?;
            self.cache = CacheDisposition::Clean;
        }
        Ok(self)
    }
}

#[cfg(test)]
pub(super) fn load_cached(root: &Path) -> Option<SourceGraphBuild> {
    let files = RepositoryFiles::open(root).ok()?;
    load_cached_from(&files)
}

pub(super) fn load_cached_from(files: &RepositoryFiles) -> Option<SourceGraphBuild> {
    #[cfg(test)]
    record_work(|counts| counts.cache_loads += 1);

    let contents = files
        .read_optional_string(Path::new(".criv/source-graph.json"))
        .ok()
        .flatten()?;
    let mut cache = serde_json::from_str::<GraphCacheFile>(&contents).ok()?;
    cache.graph.rebuild_elixir_relationships();
    (cache.schema == GRAPH_CACHE_SCHEMA).then_some(SourceGraphBuild {
        graph: cache.graph,
        cache: CacheDisposition::Clean,
    })
}

fn store_cached(files: &RepositoryFiles, graph: &SourceGraph) -> Result<()> {
    let cache = BorrowedGraphCacheFile {
        schema: GRAPH_CACHE_SCHEMA,
        graph,
    };
    #[cfg(test)]
    record_work(|counts| counts.cache_serializations += 1);
    let contents = serde_json::to_string_pretty(&cache)
        .map_err(|err| CrivError::new(format!("failed to serialize source graph cache: {err}")))?;
    let contents = format!("{contents}\n");
    files
        .write_scope(Path::new(".criv"))?
        .write_atomic(Path::new(".criv/source-graph.json"), &contents)?;
    #[cfg(test)]
    record_work(|counts| {
        counts.cache_publications += 1;
        counts.published_bytes += contents.len();
    });
    Ok(())
}

#[cfg(test)]
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

        Self {
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
        if self.language == Language::Elixir {
            format!(
                "language={}\nkind={}\nname={}\nvisibility={}\narity={}\ninputs={}\noutput={}\nguards={}\ndefaults={}\nspecifications={}\nfields={field_text}\nvariants={variant_text}\n",
                self.language.as_str(),
                self.symbol_kind.as_str(),
                self.qualified_name,
                self.visibility.as_deref().unwrap_or(""),
                self.arity
                    .map_or_else(String::new, |arity| arity.to_string()),
                self.inputs.join(","),
                self.output.as_deref().unwrap_or(""),
                self.guards.join(","),
                self.defaults.join(","),
                self.specifications.join("\n"),
            )
        } else {
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
}

impl SourceGraph {
    #[cfg(test)]
    fn build_incremental(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::build_incremental_with_workers(
            &files,
            source_files,
            previous,
            source_worker_count(source_files.len()),
        )
    }

    fn build_incremental_with_workers(
        files: &RepositoryFiles,
        source_files: &[String],
        previous: Option<&Self>,
        workers: usize,
    ) -> Result<Self> {
        let mut graph = Self::default();
        for processed in process_source_files(files, source_files, previous, workers)? {
            #[cfg(test)]
            record_work(|counts| counts.source_reads += 1);
            let Some(processed) = processed else {
                continue;
            };
            if processed.reused {
                #[cfg(test)]
                record_work(|counts| counts.reused_files += 1);
            } else {
                #[cfg(test)]
                record_work(|counts| counts.parsed_files += 1);
                graph.changed_files.push(processed.path.clone());
            }
            for symbol in &processed.parsed.symbols {
                if let Some(owner) = &symbol.owner {
                    debug_assert_eq!(
                        elixir::symbol_selector(symbol.kind, owner, &symbol.name, symbol.arity)
                            .as_deref(),
                        Some(symbol.id.selector.as_str()),
                        "structured Source owner and selector must agree"
                    );
                }
                for key in elixir::compatibility_aliases(symbol)
                    .into_iter()
                    .chain(std::iter::once(symbol.id.selector.clone()))
                {
                    graph
                        .symbol_index
                        .entry(key)
                        .or_default()
                        .push(symbol.id.clone());
                }
            }
            graph
                .file_fingerprints
                .insert(processed.path.clone(), processed.fingerprint);
            graph.files.insert(processed.path, processed.parsed);
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
        graph.rebuild_elixir_relationships();
        Ok(graph)
    }

    fn rebuild_elixir_relationships(&mut self) {
        self.elixir_relationships = elixir::ElixirRelationships::build(&self.files);
    }

    fn resolve_symbol_result(&self, query: &str) -> SymbolResolution {
        let (path, selector) = query.split_once('#').unwrap_or(("", query));
        if !path.is_empty() {
            let Some(file) = self.files.get(path) else {
                return SymbolResolution::Missing;
            };
            let exact = file
                .symbols
                .iter()
                .filter(|symbol| symbol.id.selector == selector)
                .map(|symbol| symbol.id.clone())
                .collect::<Vec<_>>();
            if !exact.is_empty() {
                return symbol_resolution(exact);
            }
            let aliases = file
                .symbols
                .iter()
                .filter(|symbol| {
                    elixir::compatibility_aliases(symbol)
                        .iter()
                        .any(|alias| alias == selector)
                })
                .map(|symbol| symbol.id.clone())
                .collect::<Vec<_>>();
            return symbol_resolution(aliases);
        }

        symbol_resolution(self.symbol_index.get(selector).cloned().unwrap_or_default())
    }

    pub(crate) fn resolve_symbol(&self, query: &str) -> Option<SymbolId> {
        match self.resolve_symbol_result(query) {
            SymbolResolution::Resolved(id) => Some(id),
            SymbolResolution::Ambiguous(_) | SymbolResolution::Missing => None,
        }
    }

    pub(crate) fn callees(&self, query: &str) -> Vec<String> {
        let Some(symbol_id) = self.resolve_symbol(query) else {
            return Vec::new();
        };
        let mut rows = self
            .symbol(&symbol_id)
            .map(|symbol| {
                let mut rows = symbol
                    .calls
                    .iter()
                    .map(|call| {
                        self.resolve_call(&symbol.id, &call.target)
                            .map_or_else(|| call.target.clone(), |id| id.display())
                    })
                    .collect::<Vec<_>>();
                rows.extend(
                    symbol
                        .relationships
                        .iter()
                        .filter(|relationship| elixir::is_executable_query_edge(relationship.kind))
                        .map(|relationship| {
                            self.resolve_relationship(&symbol.id, relationship)
                                .map_or_else(
                                    || self.relationship_target_label(&symbol.id, relationship),
                                    |id| id.display(),
                                )
                        }),
                );
                rows
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
                }) || symbol.relationships.iter().any(|relationship| {
                    elixir::is_executable_query_edge(relationship.kind)
                        && self.resolve_relationship(&symbol.id, relationship).as_ref()
                            == Some(&symbol_id)
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
                for relationship in &symbol.relationships {
                    if elixir::is_executable_query_edge(relationship.kind)
                        && let Some(target) = self.resolve_relationship(&symbol.id, relationship)
                    {
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
                if symbol.owner.is_some() {
                    symbol.exported
                } else {
                    symbol.exported
                        || symbol.name == "main"
                        || (symbol.parent.is_none() && !called.contains(&symbol.id))
                }
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

    pub(crate) fn resolve_relationship(
        &self,
        caller: &SymbolId,
        relationship: &Relationship,
    ) -> Option<SymbolId> {
        self.elixir_relationships
            .resolve(&self.files, caller, relationship)
    }

    pub(crate) fn relationship_target_label(
        &self,
        caller: &SymbolId,
        relationship: &Relationship,
    ) -> String {
        self.elixir_relationships
            .target_label(&self.files, caller, relationship)
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

    pub(crate) fn symbol_name(&self, id: &SymbolId) -> Option<&str> {
        self.symbol(id).map(|symbol| symbol.name.as_str())
    }
}

#[derive(Debug)]
struct ProcessedSource {
    path: String,
    fingerprint: String,
    parsed: SourceFile,
    reused: bool,
}

fn process_source_files(
    files: &RepositoryFiles,
    source_files: &[String],
    previous: Option<&SourceGraph>,
    workers: usize,
) -> Result<Vec<Option<ProcessedSource>>> {
    if source_files.is_empty() {
        return Ok(Vec::new());
    }
    if workers == 1 {
        return source_files
            .iter()
            .map(|source_file| process_source_file(files, source_file, previous))
            .collect();
    }

    let chunk_size = source_files.len().div_ceil(workers);
    thread::scope(|scope| {
        let handles = source_files
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|source_file| process_source_file(files, source_file, previous))
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect::<Vec<_>>();
        let mut processed = Vec::with_capacity(source_files.len());
        for handle in handles {
            processed.extend(
                handle
                    .join()
                    .map_err(|_| CrivError::new("Source worker failed"))??,
            );
        }
        Ok(processed)
    })
}

fn source_worker_count(source_files: usize) -> usize {
    let useful_workers = source_files.div_ceil(MIN_FILES_PER_SOURCE_WORKER).max(1);
    thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(MAX_SOURCE_WORKERS)
        .min(useful_workers)
}

fn process_source_file(
    files: &RepositoryFiles,
    source_file: &str,
    previous: Option<&SourceGraph>,
) -> Result<Option<ProcessedSource>> {
    let bytes = read_source_bytes(files, source_file).map_err(|error| {
        CrivError::new(format!(
            "failed to read selected file `{source_file}`: {error}"
        ))
    })?;
    if !content_inspector::inspect(&bytes).is_text() {
        return Ok(None);
    }
    let fingerprint = blake3::hash(&bytes).to_hex().to_string();
    let reused = previous
        .filter(|previous| previous.file_fingerprints.get(source_file) == Some(&fingerprint))
        .and_then(|previous| previous.files.get(source_file).cloned());
    let (parsed, reused) = if let Some(parsed) = reused {
        (parsed, true)
    } else {
        let contents = String::from_utf8_lossy(&bytes);
        (parse_source_file(source_file, &contents), false)
    };
    Ok(Some(ProcessedSource {
        path: source_file.to_string(),
        fingerprint,
        parsed,
        reused,
    }))
}

fn parse_source_file(path: &str, contents: &str) -> SourceFile {
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

fn symbol_resolution(mut matches: Vec<SymbolId>) -> SymbolResolution {
    matches.sort();
    matches.dedup();
    match matches.len() {
        0 => SymbolResolution::Missing,
        1 => SymbolResolution::Resolved(matches.pop().expect("one symbol match")),
        _ => SymbolResolution::Ambiguous(matches),
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
        Language::Elixir => Some(tree_sitter_elixir::LANGUAGE.into()),
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
        file.imports
            .push(Import::legacy(import, node.start_position().row + 1));
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

    let range = SymbolRange {
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    };
    Some(Symbol {
        id: SymbolId {
            path: path.into(),
            name: name.clone(),
            selector,
        },
        name,
        kind,
        parent: parent.map(str::to_string),
        owner: None,
        arity: None,
        exported: tree_sitter_exported(node, contents, language),
        interface_signature,
        range,
        clause_ranges: vec![range],
        calls: tree_sitter_calls(node, contents, language),
        relationships: Vec::new(),
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
        Language::Elixir | Language::Unknown => false,
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
        Language::Python | Language::Go | Language::Elixir | Language::Unknown => "public".into(),
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
                owner: None,
                arity: None,
                exported: is_exported_symbol(trimmed, language),
                range: SymbolRange {
                    start_line: line_no,
                    end_line: total_lines,
                },
                clause_ranges: vec![SymbolRange {
                    start_line: line_no,
                    end_line: total_lines,
                }],
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
        Language::Elixir | Language::Unknown => Vec::new(),
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
        Language::Elixir | Language::Unknown => None,
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
        Language::Elixir | Language::Unknown => false,
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
            Some("ex") | Some("exs") => Self::Elixir,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn graph_with_file(file: SourceFile) -> SourceGraph {
        let mut graph = SourceGraph::default();
        for symbol in &file.symbols {
            for key in elixir::compatibility_aliases(symbol)
                .into_iter()
                .chain(std::iter::once(symbol.id.selector.clone()))
            {
                graph
                    .symbol_index
                    .entry(key)
                    .or_default()
                    .push(symbol.id.clone());
            }
        }
        graph.files.insert(file.path.clone(), file);
        graph.rebuild_elixir_relationships();
        graph
    }

    fn elixir_symbol(
        path: &str,
        owner: SymbolOwner,
        kind: SymbolKind,
        name: &str,
        arity: Option<usize>,
        ranges: Vec<SymbolRange>,
    ) -> Symbol {
        let selector = elixir::symbol_selector(kind, &owner, name, arity).unwrap();
        Symbol {
            id: SymbolId {
                path: path.into(),
                name: name.into(),
                selector,
            },
            name: name.into(),
            kind,
            parent: None,
            owner: Some(owner),
            arity,
            exported: true,
            interface_signature: None,
            range: ranges[0],
            clause_ranges: ranges,
            calls: Vec::new(),
            relationships: Vec::new(),
        }
    }

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

        assert!(
            error.to_string().contains("symlinked vault path component"),
            "{error}"
        );
    }

    #[test]
    fn parallel_source_build_matches_serial_graph_order_and_cache() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        for (path, contents) in [
            ("src/a.rs", "pub fn rust_symbol() {}\n"),
            ("src/b.ts", "export function tsSymbol() {}\n"),
            ("src/c.mjs", "export function jsSymbol() {}\n"),
            ("src/d.py", "def python_symbol():\n    return 1\n"),
            ("src/e.go", "package sample\nfunc GoSymbol() {}\n"),
            (
                "src/f.ex",
                "defmodule Sample do\n  def elixir_symbol(), do: :ok\nend\n",
            ),
            (
                "src/g.exs",
                "defmodule SampleTest do\n  def test_symbol(), do: :ok\nend\n",
            ),
        ] {
            fs::write(root.path().join(path), contents).unwrap();
        }
        fs::write(root.path().join("src/h.rs"), b"\0binary").unwrap();
        let paths = [
            "src/g.exs",
            "src/h.rs",
            "src/a.rs",
            "src/f.ex",
            "src/c.mjs",
            "src/e.go",
            "src/b.ts",
            "src/d.py",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

        let serial = SourceGraphBuild::build_incremental_with_workers(root.path(), &paths, None, 1)
            .unwrap()
            .publish(root.path())
            .unwrap();
        let serial_cache = fs::read(graph_cache_path(root.path())).unwrap();
        let parallel =
            SourceGraphBuild::build_incremental_with_workers(root.path(), &paths, None, 4)
                .unwrap()
                .publish(root.path())
                .unwrap();
        let parallel_cache = fs::read(graph_cache_path(root.path())).unwrap();

        assert_eq!(parallel.graph(), serial.graph());
        assert_eq!(parallel.paths(), serial.paths());
        assert_eq!(
            parallel.paths(),
            vec![
                "src/a.rs",
                "src/b.ts",
                "src/c.mjs",
                "src/d.py",
                "src/e.go",
                "src/f.ex",
                "src/g.exs",
            ]
        );
        assert_eq!(
            parallel.graph().changed_files(),
            [
                "src/g.exs",
                "src/a.rs",
                "src/f.ex",
                "src/c.mjs",
                "src/e.go",
                "src/b.ts",
                "src/d.py",
            ]
        );
        assert_eq!(parallel_cache, serial_cache);

        let serial_cached =
            SourceGraphBuild::build_incremental_with_workers(root.path(), &paths, Some(&serial), 1)
                .unwrap()
                .publish(root.path())
                .unwrap();
        let parallel_cached = SourceGraphBuild::build_incremental_with_workers(
            root.path(),
            &paths,
            Some(&parallel),
            4,
        )
        .unwrap()
        .publish(root.path())
        .unwrap();
        assert_eq!(parallel_cached.graph(), serial_cached.graph());
        assert!(parallel_cached.graph().changed_files().is_empty());
        assert_eq!(
            fs::read(graph_cache_path(root.path())).unwrap(),
            serial_cache
        );
    }

    #[test]
    fn parallel_source_build_reports_the_same_selected_read_error_as_serial() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/a.rs"), "pub fn present() {}\n").unwrap();
        fs::write(root.path().join("src/c.ex"), "defmodule Present do\nend\n").unwrap();
        let paths = vec![
            "src/a.rs".into(),
            "src/missing.rs".into(),
            "src/c.ex".into(),
        ];

        let serial = SourceGraphBuild::build_incremental_with_workers(root.path(), &paths, None, 1)
            .unwrap_err();
        let parallel =
            SourceGraphBuild::build_incremental_with_workers(root.path(), &paths, None, 3)
                .unwrap_err();

        assert_eq!(parallel.to_string(), serial.to_string());
        assert!(
            parallel
                .to_string()
                .contains("failed to read selected file `src/missing.rs`")
        );
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
    fn elixir_files_are_all_parsed_reused_and_reparsed_by_content() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("test")).unwrap();
        fs::write(
            root.join("lib/sample.ex"),
            "defmodule Sample do\n  def run(), do: :ok\n  def broken(, do: :bad)\n  def after_error(), do: :ok\nend\n",
        )
        .unwrap();
        fs::write(
            root.join("test/sample_test.exs"),
            "defmodule SampleTest do\n  def run(), do: :ok\nend\n",
        )
        .unwrap();
        let paths = vec!["lib/sample.ex".into(), "test/sample_test.exs".into()];

        reset_work_counts();
        let first = SourceGraphBuild::build_incremental(root, &paths, None).unwrap();
        assert_eq!(work_counts().source_reads, 2);
        assert_eq!(work_counts().parsed_files, 2);
        assert_eq!(
            first.graph().files["lib/sample.ex"].language,
            Language::Elixir
        );
        assert_eq!(
            first.graph().files["test/sample_test.exs"].language,
            Language::Elixir
        );
        assert!(
            first
                .graph()
                .resolve_symbol("lib/sample.ex#Sample.after_error/0")
                .is_some()
        );
        assert!(
            first
                .graph()
                .resolve_symbol("lib/sample.ex#broken")
                .is_none()
        );
        assert!(
            first
                .graph()
                .resolve_symbol("test/sample_test.exs#SampleTest.run/0")
                .is_some()
        );

        reset_work_counts();
        let second = SourceGraphBuild::build_incremental(root, &paths, Some(&first)).unwrap();
        assert_eq!(work_counts().source_reads, 2);
        assert_eq!(work_counts().reused_files, 2);
        assert_eq!(work_counts().parsed_files, 0);

        fs::write(
            root.join("test/sample_test.exs"),
            "defmodule SampleTest do\n  def changed(), do: :ok\nend\n",
        )
        .unwrap();
        reset_work_counts();
        let third = SourceGraphBuild::build_incremental(root, &paths, Some(&second)).unwrap();
        assert_eq!(work_counts().source_reads, 2);
        assert_eq!(work_counts().reused_files, 1);
        assert_eq!(work_counts().parsed_files, 1);
        assert!(
            third
                .graph()
                .resolve_symbol("test/sample_test.exs#changed/0")
                .is_some()
        );
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
            r#"{"schema":"criv.source-graph/2","graph":{}}"#,
        )
        .unwrap();
        assert!(load_cached(temp.path()).is_none());
    }

    #[test]
    fn elixir_aliases_are_unique_or_ambiguous_and_exact_lookup_wins() {
        let owner = SymbolOwner::Module {
            name: "My.App".into(),
        };
        let mut file = SourceFile {
            path: "lib/my_app.ex".into(),
            language: Language::Unknown,
            imports: Vec::new(),
            modules: Vec::new(),
            symbols: vec![
                elixir_symbol(
                    "lib/my_app.ex",
                    owner.clone(),
                    SymbolKind::Function,
                    "run",
                    Some(1),
                    vec![SymbolRange {
                        start_line: 2,
                        end_line: 3,
                    }],
                ),
                elixir_symbol(
                    "lib/my_app.ex",
                    owner,
                    SymbolKind::Function,
                    "run",
                    Some(2),
                    vec![
                        SymbolRange {
                            start_line: 5,
                            end_line: 6,
                        },
                        SymbolRange {
                            start_line: 8,
                            end_line: 9,
                        },
                    ],
                ),
            ],
        };
        file.symbols.push(Symbol {
            id: SymbolId {
                path: file.path.clone(),
                name: "module:My.App/fn:run/1".into(),
                selector: "fn:literal".into(),
            },
            name: "module:My.App/fn:run/1".into(),
            kind: SymbolKind::Function,
            parent: None,
            owner: None,
            arity: None,
            exported: false,
            interface_signature: None,
            range: SymbolRange {
                start_line: 11,
                end_line: 11,
            },
            clause_ranges: vec![SymbolRange {
                start_line: 11,
                end_line: 11,
            }],
            calls: Vec::new(),
            relationships: Vec::new(),
        });
        let graph = graph_with_file(file);

        assert!(matches!(
            graph.resolve_symbol_result("lib/my_app.ex#run"),
            SymbolResolution::Ambiguous(matches) if matches.len() == 2
        ));
        assert_eq!(
            graph
                .resolve_symbol("lib/my_app.ex#run/1")
                .unwrap()
                .display(),
            "lib/my_app.ex#module:My.App/fn:run/1"
        );
        assert_eq!(
            graph
                .resolve_symbol("lib/my_app.ex#My.App.run/2")
                .unwrap()
                .display(),
            "lib/my_app.ex#module:My.App/fn:run/2"
        );
        assert_eq!(
            graph
                .resolve_symbol("lib/my_app.ex#module:My.App/fn:run/1")
                .unwrap()
                .display(),
            "lib/my_app.ex#module:My.App/fn:run/1"
        );
        assert_eq!(
            graph.files["lib/my_app.ex"].symbols[1].clause_ranges.len(),
            2
        );
    }

    #[test]
    fn current_language_selector_text_is_unchanged() {
        let cases = [
            ("src/lib.rs", "fn run() {}", "src/lib.rs#fn:run"),
            ("src/app.ts", "function run() {}", "src/app.ts#fn:run"),
            ("src/app.js", "function run() {}", "src/app.js#fn:run"),
            ("src/app.py", "def run():\n    pass", "src/app.py#fn:run"),
            (
                "src/app.go",
                "package app\nfunc run() {}",
                "src/app.go#fn:run",
            ),
        ];
        for (path, source, expected) in cases {
            let file = parse_source_file(path, source);
            assert!(
                file.symbols
                    .iter()
                    .any(|symbol| symbol.id.display() == expected),
                "missing golden selector {expected}"
            );
        }
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
    fn elixir_relationship_resolution_is_exact_lexical_and_kind_aware() {
        let file = parse_source_file(
            "lib/relationships.ex",
            r#"
defmodule Root.Repo do
  def fetch(value), do: value
  def fetch(value, opts), do: {value, opts}
end

defmodule Root.Helpers do
  def allowed(value), do: value
  def blocked(value), do: value
  def clash(value), do: value
  def local(value), do: value
end

defmodule Other.Helpers do
  def clash(value), do: value
end

defmodule Root.Macros do
  defmacro build(value), do: value
end

defmodule Root.FunctionClasses do
  def function_only(value), do: value
  defmacro macro_blocked(value), do: value
end

defmodule Root.MacroClasses do
  def function_blocked(value), do: value
  defmacro macro_allowed(value), do: value
end

defmodule Root.SigilClasses do
  defmacro sigil_q(value), do: value
  defmacro regular_macro(value), do: value
end

defmodule Root.Behaviour do
  @callback run(term()) :: term()
end

defprotocol Root.Proto do
  def work(value)
end

defmodule Unrelated do
  def missing(value), do: value
end

defmodule My.App do
  alias Root.Repo
  require Root.Macros, as: M
  import Root.Helpers,
    only: [allowed: 1, blocked: 1, clash: 1, local: 1],
    except: [blocked: 1]
  import Other.Helpers, only: [clash: 1]
  import Root.FunctionClasses, only: :functions
  import Root.MacroClasses, only: :macros
  import Root.SigilClasses, only: :sigils
  @behaviour Root.Behaviour
  alias Other.Helpers, as: Root

  def run(value) do
    local(value)
    allowed(value)
    blocked(value)
    clash(value)
    missing(value)
    value |> Repo.fetch(opt())
    M.build(value)
    function_only(value)
    macro_blocked(value)
    macro_allowed(value)
    function_blocked(value)
    sigil_q(value)
    regular_macro(value)
    __MODULE__.local(value)
    Elixir.Root.Repo.fetch(value)
    :lists.reverse(value)
    apply(Repo, :fetch, [value])
    fun.(value)
    quote do
      hidden()
    end
  end

  def local(value), do: value
  defdelegate delegated(value), to: Repo, as: :fetch
  def capture_only(), do: &Repo.fetch/1

  def scoped(value) do
    alias Elixir.Root.Repo, as: Scoped
    Scoped.fetch(value)
  end

  def outside(value), do: Scoped.fetch(value)
  def same_line(value), do: (if value, do: (alias Elixir.Root.Repo, as: Here; Here.fetch(value)); Here.fetch(value))
end

defimpl Root.Proto, for: My.App do
  def work(value), do: value
end
"#,
        );
        let graph = graph_with_file(file);

        let run = graph.callees("lib/relationships.ex#My.App.run/1");
        for target in [
            "lib/relationships.ex#module:My.App/fn:local/1",
            "lib/relationships.ex#module:Root.Helpers/fn:allowed/1",
            "lib/relationships.ex#module:Root.Repo/fn:fetch/1",
            "lib/relationships.ex#module:Root.Repo/fn:fetch/2",
            "lib/relationships.ex#module:Root.Macros/macro:build/1",
            "lib/relationships.ex#module:Root.FunctionClasses/fn:function_only/1",
            "lib/relationships.ex#module:Root.MacroClasses/macro:macro_allowed/1",
            "lib/relationships.ex#module:Root.SigilClasses/macro:sigil_q/1",
            "blocked/1",
            "clash/1",
            "function_blocked/1",
            "macro_blocked/1",
            "missing/1",
            ":lists.reverse/1",
            "regular_macro/1",
        ] {
            assert!(
                run.iter().any(|row| row == target),
                "missing {target}: {run:?}"
            );
        }
        assert!(!run.iter().any(|row| row.contains("hidden")));
        assert!(run.iter().any(|row| row.starts_with("dynamic:")));

        assert_eq!(
            graph.callees("lib/relationships.ex#My.App.delegated/1"),
            vec!["lib/relationships.ex#module:Root.Repo/fn:fetch/1"]
        );
        let callers = graph.callers("lib/relationships.ex#Root.Repo.fetch/1");
        assert!(callers.contains(&"lib/relationships.ex#module:My.App/fn:delegated/1".to_string()));
        assert!(callers.contains(&"lib/relationships.ex#module:My.App/fn:run/1".to_string()));
        assert!(
            !callers.contains(&"lib/relationships.ex#module:My.App/fn:capture_only/0".to_string())
        );

        assert_eq!(
            graph.callees("lib/relationships.ex#My.App.scoped/1"),
            vec!["lib/relationships.ex#module:Root.Repo/fn:fetch/1"]
        );
        assert_eq!(
            graph.callees("lib/relationships.ex#My.App.outside/1"),
            vec!["Scoped.fetch/1"]
        );
        assert_eq!(
            graph.callees("lib/relationships.ex#My.App.same_line/1"),
            vec![
                "Here.fetch/1",
                "lib/relationships.ex#module:Root.Repo/fn:fetch/1",
            ]
        );

        let app = graph
            .resolve_symbol("lib/relationships.ex#module:My.App")
            .unwrap();
        let app_symbol = graph.symbol(&app).unwrap();
        let behaviour = app_symbol
            .relationships
            .iter()
            .find(|relationship| relationship.kind == RelationshipKind::BehaviourImplementation)
            .unwrap();
        assert_eq!(
            graph
                .resolve_relationship(&app, behaviour)
                .unwrap()
                .display(),
            "lib/relationships.ex#module:Root.Behaviour"
        );

        let implementation = graph
            .resolve_symbol("lib/relationships.ex#impl:Root.Proto/for:My.App")
            .unwrap();
        let implementation_symbol = graph.symbol(&implementation).unwrap();
        let targets = implementation_symbol
            .relationships
            .iter()
            .map(|relationship| {
                graph
                    .resolve_relationship(&implementation, relationship)
                    .unwrap()
                    .display()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            targets,
            BTreeSet::from([
                "lib/relationships.ex#module:My.App".to_string(),
                "lib/relationships.ex#module:Root.Proto".to_string(),
            ])
        );
    }

    #[test]
    fn elixir_relationships_restore_from_cache_and_keep_cross_file_ambiguity() {
        fn resolved_target(graph: &SourceGraph) -> Option<String> {
            let caller = graph
                .resolve_symbol("lib/caller.ex#module:Caller/fn:run/0")
                .unwrap();
            let relationship = graph
                .symbol(&caller)
                .unwrap()
                .relationships
                .first()
                .unwrap();
            graph
                .resolve_relationship(&caller, relationship)
                .map(|target| target.display())
        }

        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("lib")).unwrap();
        fs::write(
            root.path().join("lib/caller.ex"),
            "defmodule Caller do\n  def run(), do: Target.run()\nend\n",
        )
        .unwrap();
        fs::write(
            root.path().join("lib/target.ex"),
            "defmodule Target do\n  def run(), do: :ok\nend\n",
        )
        .unwrap();
        let paths = vec!["lib/caller.ex".into(), "lib/target.ex".into()];
        let built = SourceGraphBuild::build_incremental(root.path(), &paths, None).unwrap();
        assert_eq!(
            resolved_target(built.graph()),
            Some("lib/target.ex#module:Target/fn:run/0".into())
        );

        built.publish(root.path()).unwrap();
        let loaded = load_cached(root.path()).unwrap();
        assert_eq!(
            resolved_target(loaded.graph()),
            Some("lib/target.ex#module:Target/fn:run/0".into())
        );

        fs::write(
            root.path().join("lib/duplicate.ex"),
            "defmodule Target do\n  def run(), do: :duplicate\nend\n",
        )
        .unwrap();
        let paths = vec![
            "lib/caller.ex".into(),
            "lib/duplicate.ex".into(),
            "lib/target.ex".into(),
        ];
        let ambiguous =
            SourceGraphBuild::build_incremental(root.path(), &paths, Some(&loaded)).unwrap();
        assert_eq!(resolved_target(ambiguous.graph()), None);
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

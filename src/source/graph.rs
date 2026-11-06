use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::thread;

use serde::{Deserialize, Serialize};

use criv_state_wire::layout;
use criv_state_wire::source_identity::{self, SourceIdentity, SourceSelector};

use crate::repository::RepositoryFiles;
#[cfg(test)]
use crate::work::{self, Work};
use crate::{CrivError, Result};

mod elixir;
mod extract;
mod syntax;

// Bump when parsed source graph semantics or serialized graph types change.
const GRAPH_CACHE_SCHEMA: &str = "criv.source-graph/4";
const MAX_SOURCE_WORKERS: usize = 16;
const MIN_FILES_PER_SOURCE_WORKER: usize = 1_024;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceGraph {
    files: BTreeMap<String, SourceFile>,
    file_fingerprints: BTreeMap<String, String>,
    #[serde(skip)]
    changed_files: Vec<String>,
    symbol_index: BTreeMap<String, Vec<SymbolId>>,
    #[serde(skip)]
    elixir_relationships: elixir::ElixirRelationships,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub(crate) path: String,
    pub(crate) language: Language,
    pub(crate) imports: Vec<Import>,
    modules: Vec<ModuleDecl>,
    pub(crate) symbols: Vec<Symbol>,
}

impl SourceFile {
    fn new(path: &str, language: Language) -> Self {
        Self {
            path: path.into(),
            language,
            imports: Vec::new(),
            modules: Vec::new(),
            symbols: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDecl {
    name: String,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub(crate) module: String,
    pub(crate) line: usize,
    pub(crate) site: usize,
    pub(crate) kind: DirectiveKind,
    pub(crate) owner: Option<SymbolOwner>,
    pub(crate) scope: Option<LexicalScope>,
    pub(crate) alias: Option<String>,
    pub(crate) only: Option<DirectiveFilter>,
    pub(crate) except: Vec<CallableFilter>,
    pub(crate) absolute: bool,
}

impl Import {
    const fn legacy(module: String, line: usize) -> Self {
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
pub struct LexicalScope {
    start_byte: usize,
    end_byte: usize,
}

#[derive(Debug, Default, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum DirectiveKind {
    Alias,
    Import,
    Require,
    Use,
    #[default]
    Legacy,
}

impl DirectiveKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Alias => "alias",
            Self::Import | Self::Legacy => "import",
            Self::Require => "require",
            Self::Use => "use",
        }
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum DirectiveFilter {
    Callables(Vec<CallableFilter>),
    Functions,
    Macros,
    Sigils,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CallableFilter {
    name: String,
    arity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub(crate) id: SymbolId,
    pub(crate) name: String,
    pub(crate) kind: SymbolKind,
    pub(crate) parent: Option<String>,
    pub(crate) owner: Option<SymbolOwner>,
    pub(crate) arity: Option<usize>,
    exported: bool,
    interface_signature: Option<InterfaceSignature>,
    pub(crate) range: SymbolRange,
    clause_ranges: Vec<SymbolRange>,
    pub(crate) calls: Vec<Call>,
    pub(crate) relationships: Vec<Relationship>,
}

impl Symbol {
    /// A private, unowned symbol with one clause at `range` and no edges.
    fn new(
        path: &str,
        name: String,
        kind: SymbolKind,
        selector: String,
        range: SymbolRange,
    ) -> Self {
        Self {
            id: SymbolId {
                path: path.into(),
                name: name.clone(),
                selector,
            },
            name,
            kind,
            parent: None,
            owner: None,
            arity: None,
            exported: false,
            interface_signature: None,
            range,
            clause_ranges: vec![range],
            calls: Vec::new(),
            relationships: Vec::new(),
        }
    }

    fn compatibility_aliases(&self) -> Vec<String> {
        let module = match &self.owner {
            Some(SymbolOwner::Module { name }) => Some(name.as_str()),
            Some(SymbolOwner::Implementation { .. }) | None => None,
        };
        source_identity::compatibility_aliases(&self.name, self.arity, module)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymbolRange {
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum SymbolOwner {
    Module { name: String },
    Implementation { protocol: String, for_type: String },
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct SymbolId {
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
pub struct Call {
    pub(crate) target: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum RelationshipKind {
    Call,
    Capture,
    Delegate,
    ProtocolImplementation,
    BehaviourImplementation,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Relationship {
    pub(crate) kind: RelationshipKind,
    pub(crate) target: RelationshipTarget,
    pub(crate) line: usize,
    pub(crate) site: usize,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ModuleRelationshipRole {
    Protocol,
    ForType,
    Behaviour,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum RelationshipTarget {
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

/// A call or relationship of one symbol.
#[derive(Debug, Clone, Copy)]
pub enum EdgeOrigin<'a> {
    Call(&'a Call),
    Relationship(&'a Relationship),
}

/// One outgoing edge of a symbol and the graph symbol it resolves to.
#[derive(Debug, Clone)]
pub struct ResolvedEdge<'a> {
    pub(crate) origin: EdgeOrigin<'a>,
    pub(crate) target: Option<SymbolId>,
}

impl ResolvedEdge<'_> {
    /// Callers, callees, and the attack surface follow only executable edges.
    const fn is_executable(&self) -> bool {
        match self.origin {
            EdgeOrigin::Call(_) => true,
            EdgeOrigin::Relationship(relationship) => {
                elixir::is_executable_query_edge(relationship.kind)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum SymbolKind {
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
    pub(crate) const fn as_str(self) -> &'static str {
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
pub enum SymbolResolution {
    Resolved(SymbolId),
    Ambiguous(Vec<SymbolId>),
    Missing,
}

#[derive(Debug, Default, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Elixir,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct InterfaceSignature {
    language: Language,
    symbol_kind: SymbolKind,
    qualified_name: String,
    visibility: Option<String>,
    inputs: Vec<String>,
    output: Option<String>,
    fields: Vec<FieldSignature>,
    variants: Vec<VariantSignature>,
    arity: Option<usize>,
    guards: Vec<String>,
    defaults: Vec<String>,
    specifications: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct FieldSignature {
    name: String,
    ty: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct VariantSignature {
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
    pub(super) fn build(
        files: &RepositoryFiles,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        Self::build_with_workers(
            files,
            source_files,
            previous,
            source_worker_count(source_files.len()),
        )
    }

    fn build_with_workers(
        files: &RepositoryFiles,
        source_files: &[String],
        previous: Option<&Self>,
        workers: usize,
    ) -> Result<Self> {
        let graph = SourceGraph::build(files, source_files, previous.map(Self::graph), workers)?;
        let can_reuse = previous.is_some_and(|previous| {
            previous.cache == CacheDisposition::Clean && graph.changed_files().is_empty()
        });
        let cache = if can_reuse && files.file_exists(Path::new(layout::SOURCE_GRAPH))? {
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

    pub(super) const fn graph(&self) -> &SourceGraph {
        &self.graph
    }

    pub(super) fn paths(&self) -> Vec<String> {
        self.graph.files.keys().cloned().collect()
    }

    pub(super) fn publish(mut self, files: &RepositoryFiles) -> Result<Self> {
        if self.cache == CacheDisposition::Dirty {
            store_cached(files, &self.graph)?;
            self.cache = CacheDisposition::Clean;
        }
        Ok(self)
    }
}

/// The published graph cache, or `None` when it is missing, unreadable as a
/// cache, or from another schema.
pub(super) fn load_cached(files: &RepositoryFiles) -> Result<Option<SourceGraphBuild>> {
    #[cfg(test)]
    work::record(Work::GraphCacheLoad);

    let Some((contents, _)) =
        files.read_optional_with_permissions(Path::new(layout::SOURCE_GRAPH))?
    else {
        return Ok(None);
    };
    let Ok(cache) = serde_json::from_slice::<GraphCacheFile>(&contents) else {
        return Ok(None);
    };
    if cache.schema != GRAPH_CACHE_SCHEMA {
        return Ok(None);
    }
    let mut graph = cache.graph;
    graph.rebuild_elixir_relationships();
    Ok(Some(SourceGraphBuild {
        graph,
        cache: CacheDisposition::Clean,
    }))
}

fn store_cached(files: &RepositoryFiles, graph: &SourceGraph) -> Result<()> {
    let cache = BorrowedGraphCacheFile {
        schema: GRAPH_CACHE_SCHEMA,
        graph,
    };
    #[cfg(test)]
    work::record(Work::GraphCacheSerialization);
    let contents = serde_json::to_string_pretty(&cache)
        .map_err(|err| CrivError::new(format!("failed to serialize source graph cache: {err}")))?;
    let contents = format!("{contents}\n");
    files
        .write_scope(Path::new(layout::DIR))?
        .write_atomic(Path::new(layout::SOURCE_GRAPH), &contents)?;
    #[cfg(test)]
    {
        work::record(Work::GraphCachePublication);
        work::add(Work::GraphCacheBytes, contents.len());
    }
    Ok(())
}

impl SourceGraph {
    fn build(
        files: &RepositoryFiles,
        source_files: &[String],
        previous: Option<&Self>,
        workers: usize,
    ) -> Result<Self> {
        let mut graph = Self::default();
        for processed in process_source_files(files, source_files, previous, workers)? {
            #[cfg(test)]
            work::record(Work::SourceRead);
            let Some(processed) = processed else {
                continue;
            };
            if processed.reused {
                #[cfg(test)]
                work::record(Work::SourceReuse);
            } else {
                #[cfg(test)]
                work::record(Work::SourceParse);
                graph.changed_files.push(processed.parsed.path.clone());
            }
            graph.insert(processed.parsed, processed.fingerprint);
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

    /// Adds a parsed file and indexes its symbols by selector and alias.
    fn insert(&mut self, file: SourceFile, fingerprint: String) {
        for symbol in &file.symbols {
            for key in symbol
                .compatibility_aliases()
                .into_iter()
                .chain(std::iter::once(symbol.id.selector.clone()))
            {
                self.symbol_index
                    .entry(key)
                    .or_default()
                    .push(symbol.id.clone());
            }
        }
        self.file_fingerprints
            .insert(file.path.clone(), fingerprint);
        self.files.insert(file.path.clone(), file);
    }

    fn rebuild_elixir_relationships(&mut self) {
        self.elixir_relationships = elixir::ElixirRelationships::build(&self.files);
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.values()
    }

    pub(crate) fn file(&self, path: &str) -> Option<&SourceFile> {
        self.files.get(path)
    }

    /// The BLAKE3 hash of the bytes the file was parsed from.
    pub(crate) fn file_fingerprint(&self, path: &str) -> Option<&str> {
        self.file_fingerprints.get(path).map(String::as_str)
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
                    symbol
                        .compatibility_aliases()
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

    /// Every call and relationship of `symbol`, calls first, each with the
    /// graph symbol it resolves to.
    pub(crate) fn resolved_edges<'a>(
        &'a self,
        symbol: &'a Symbol,
    ) -> impl Iterator<Item = ResolvedEdge<'a>> + 'a {
        let calls = symbol.calls.iter().map(|call| ResolvedEdge {
            origin: EdgeOrigin::Call(call),
            target: self.resolve_call(&symbol.id, &call.target),
        });
        let relationships = symbol
            .relationships
            .iter()
            .map(|relationship| ResolvedEdge {
                origin: EdgeOrigin::Relationship(relationship),
                target: self.resolve_relationship(&symbol.id, relationship),
            });
        calls.chain(relationships)
    }

    pub(crate) fn callees(&self, query: &str) -> Vec<String> {
        let Some(symbol) = self
            .resolve_symbol(query)
            .and_then(|symbol_id| self.symbol(&symbol_id))
        else {
            return Vec::new();
        };
        let mut rows = self
            .resolved_edges(symbol)
            .filter(ResolvedEdge::is_executable)
            .map(|edge| match (edge.target, edge.origin) {
                (Some(target), _) => target.display(),
                (None, EdgeOrigin::Call(call)) => call.target.clone(),
                (None, EdgeOrigin::Relationship(relationship)) => {
                    self.relationship_target_label(&symbol.id, relationship)
                }
            })
            .collect::<Vec<_>>();
        rows.sort();
        rows.dedup();
        rows
    }

    /// Symbols with an executable edge to the queried symbol. An unresolved
    /// call counts when its name matches, so an ambiguous name lists every
    /// possible caller; a call resolved to another symbol does not count.
    pub(crate) fn callers(&self, query: &str) -> Vec<String> {
        let Some(symbol_id) = self.resolve_symbol(query) else {
            return Vec::new();
        };
        let mut rows = self
            .symbols()
            .filter(|caller| {
                self.resolved_edges(caller)
                    .filter(ResolvedEdge::is_executable)
                    .any(|edge| match (&edge.target, edge.origin) {
                        (Some(target), _) => *target == symbol_id,
                        (None, EdgeOrigin::Call(call)) => call.target == symbol_id.name,
                        (None, EdgeOrigin::Relationship(_)) => false,
                    })
            })
            .map(|caller| caller.id.display())
            .collect::<Vec<_>>();
        rows.sort();
        rows.dedup();
        rows
    }

    pub(crate) fn attack_surface(&self) -> Vec<String> {
        let called = self
            .symbols()
            .flat_map(|symbol| self.resolved_edges(symbol))
            .filter(ResolvedEdge::is_executable)
            .filter_map(|edge| edge.target)
            .collect::<BTreeSet<_>>();
        let called = &called;

        let mut rows = self
            .files
            .values()
            .flat_map(|file| {
                let implicit = file.language.has_implicit_entry_points();
                file.symbols.iter().filter(move |symbol| {
                    symbol.exported
                        || (implicit
                            && (symbol.name == "main"
                                || (symbol.parent.is_none() && !called.contains(&symbol.id))))
                })
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

    fn resolve_call(&self, caller: &SymbolId, target: &str) -> Option<SymbolId> {
        self.files
            .get(&caller.path)
            .and_then(|file| file.symbols.iter().find(|symbol| symbol.name == target))
            .map(|symbol| symbol.id.clone())
            .or_else(|| self.resolve_symbol(target))
    }

    fn resolve_relationship(
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
        elixir::ElixirRelationships::target_label(&self.files, caller, relationship)
    }

    pub(crate) fn canonical_symbol_target(&self, query: &str) -> Option<String> {
        self.resolve_symbol(query).map(|symbol| symbol.display())
    }

    fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        symbol_in(&self.files, id)
    }

    pub(crate) fn symbol_name(&self, id: &SymbolId) -> Option<&str> {
        self.symbol(id).map(|symbol| symbol.name.as_str())
    }
}

fn symbol_in<'a>(files: &'a BTreeMap<String, SourceFile>, id: &SymbolId) -> Option<&'a Symbol> {
    files
        .get(&id.path)
        .and_then(|file| file.symbols.iter().find(|symbol| &symbol.id == id))
}

#[derive(Debug)]
struct ProcessedSource {
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
    if workers <= 1 {
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
        .map_or(1, usize::from)
        .min(MAX_SOURCE_WORKERS)
        .min(useful_workers)
}

fn process_source_file(
    files: &RepositoryFiles,
    source_file: &str,
    previous: Option<&SourceGraph>,
) -> Result<Option<ProcessedSource>> {
    let bytes = files.read(Path::new(source_file)).map_err(|error| {
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
    let (parsed, reused) = reused.map_or_else(
        || {
            let contents = String::from_utf8_lossy(&bytes);
            (extract::parse_source_file(source_file, &contents), false)
        },
        |parsed| (parsed, true),
    );
    Ok(Some(ProcessedSource {
        fingerprint,
        parsed,
        reused,
    }))
}

fn symbol_resolution(mut matches: Vec<SymbolId>) -> SymbolResolution {
    matches.sort();
    matches.dedup();
    match matches.len() {
        0 => SymbolResolution::Missing,
        1 => matches
            .pop()
            .map_or(SymbolResolution::Missing, SymbolResolution::Resolved),
        _ => SymbolResolution::Ambiguous(matches),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use extract::parse_source_file;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn files(root: &Path) -> RepositoryFiles {
        RepositoryFiles::open(root).unwrap()
    }

    fn build(
        root: &Path,
        paths: &[String],
        previous: Option<&SourceGraphBuild>,
    ) -> Result<SourceGraphBuild> {
        SourceGraphBuild::build(&files(root), paths, previous)
    }

    fn build_with_workers(
        root: &Path,
        paths: &[String],
        previous: Option<&SourceGraphBuild>,
        workers: usize,
    ) -> Result<SourceGraphBuild> {
        SourceGraphBuild::build_with_workers(&files(root), paths, previous, workers)
    }

    fn graph_build(
        root: &Path,
        paths: &[String],
        previous: Option<&SourceGraph>,
    ) -> Result<SourceGraph> {
        SourceGraph::build(&files(root), paths, previous, 1)
    }

    impl SourceGraphBuild {
        fn publish_at(self, root: &Path) -> Result<Self> {
            self.publish(&files(root))
        }
    }

    fn cached(root: &Path) -> Option<SourceGraphBuild> {
        load_cached(&files(root)).unwrap()
    }

    fn graph_cache_path(root: &Path) -> PathBuf {
        root.join(layout::SOURCE_GRAPH)
    }

    fn graph_with_files(files: impl IntoIterator<Item = SourceFile>) -> SourceGraph {
        let mut graph = SourceGraph::default();
        for file in files {
            graph.insert(file, String::new());
        }
        graph.rebuild_elixir_relationships();
        graph
    }

    #[test]
    fn language_detection_uses_the_file_extension() {
        let cases = [
            ("src/lib.rs", Language::Rust),
            ("web/app.tsx", Language::TypeScript),
            ("web/app.mjs", Language::JavaScript),
            ("tools/task.py", Language::Python),
            ("cmd/main.go", Language::Go),
            ("lib/app.exs", Language::Elixir),
            ("docs/guide.md", Language::Unknown),
        ];

        for (path, language) in cases {
            assert_eq!(Language::from_path(path), language, "{path}");
        }
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
            owner: Some(owner),
            arity,
            exported: true,
            clause_ranges: ranges.clone(),
            ..Symbol::new(path, name.into(), kind, selector, ranges[0])
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

        let error = graph_build(&root, &["src/secret.rs".into()], None)
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

        let serial = build_with_workers(root.path(), &paths, None, 1)
            .unwrap()
            .publish_at(root.path())
            .unwrap();
        let serial_cache = fs::read(graph_cache_path(root.path())).unwrap();
        let parallel = build_with_workers(root.path(), &paths, None, 4)
            .unwrap()
            .publish_at(root.path())
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

        let serial_cached = build_with_workers(root.path(), &paths, Some(&serial), 1)
            .unwrap()
            .publish_at(root.path())
            .unwrap();
        let parallel_cached = build_with_workers(root.path(), &paths, Some(&parallel), 4)
            .unwrap()
            .publish_at(root.path())
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

        let serial = build_with_workers(root.path(), &paths, None, 1).unwrap_err();
        let parallel = build_with_workers(root.path(), &paths, None, 3).unwrap_err();

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
        let mut build = build(root, &["src/lib.rs".into()], None).unwrap();
        assert_eq!(build.graph.changed_files(), &["src/lib.rs".to_string()]);
        build.graph.changed_files.push("scratch.rs".to_string());

        let build = build.publish_at(root).unwrap();
        let loaded = cached(root).unwrap();

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
        let first = build(root, &["src/lib.rs".into()], None)
            .unwrap()
            .publish_at(root)
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
        work::reset();
        build(root, &["src/lib.rs".into()], Some(&first))
            .unwrap()
            .publish_at(root)
            .unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(work::count(Work::GraphCacheSerialization), 0);
    }

    #[test]
    fn work_counts_distinguish_parse_reuse_and_cache_publication() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        work::reset();

        let first = build(root, &["src/lib.rs".into()], None)
            .unwrap()
            .publish_at(root)
            .unwrap();
        build(root, &["src/lib.rs".into()], Some(&first))
            .unwrap()
            .publish_at(root)
            .unwrap();

        assert_eq!(
            work::counts([
                Work::SourceRead,
                Work::SourceParse,
                Work::SourceReuse,
                Work::GraphCacheSerialization,
                Work::GraphCachePublication,
            ]),
            [2, 1, 1, 1, 1]
        );
        assert!(work::count(Work::GraphCacheBytes) > 0);
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

        work::reset();
        let first = build(root, &paths, None).unwrap();
        assert_eq!(work::counts([Work::SourceRead, Work::SourceParse]), [2, 2]);
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

        work::reset();
        let second = build(root, &paths, Some(&first)).unwrap();
        assert_eq!(
            work::counts([Work::SourceRead, Work::SourceReuse, Work::SourceParse]),
            [2, 2, 0]
        );

        fs::write(
            root.join("test/sample_test.exs"),
            "defmodule SampleTest do\n  def changed(), do: :ok\nend\n",
        )
        .unwrap();
        work::reset();
        let third = build(root, &paths, Some(&second)).unwrap();
        assert_eq!(
            work::counts([Work::SourceRead, Work::SourceReuse, Work::SourceParse]),
            [2, 1, 1]
        );
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
            let build = build(temp.path(), &["src/lib.rs".into()], None)
                .unwrap()
                .publish_at(temp.path())
                .unwrap();
            (temp, build)
        }

        fn publish_and_assert(
            root: &Path,
            source_files: &[String],
            previous: Option<&SourceGraphBuild>,
        ) -> SourceGraphBuild {
            work::reset();
            let build = build(root, source_files, previous)
                .unwrap()
                .publish_at(root)
                .unwrap();
            assert_eq!(
                work::counts([Work::GraphCacheSerialization, Work::GraphCachePublication]),
                [1, 1]
            );
            build
        }

        let (missing, previous) = baseline();
        fs::remove_file(graph_cache_path(missing.path())).unwrap();
        publish_and_assert(missing.path(), &["src/lib.rs".into()], Some(&previous));

        let (garbage, _) = baseline();
        fs::write(graph_cache_path(garbage.path()), "garbage\n").unwrap();
        let loaded = cached(garbage.path());
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
        let loaded = cached(wrong_schema.path());
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
        let build = build(&root, &["src/lib.rs".into()], None).unwrap();

        let error = build.publish_at(&root).unwrap_err();

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
        let before = graph_build(root, &["src/lib.rs".into()], None).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();

        fs::write(&path, "pub fn two() {}\n").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let after = graph_build(root, &["src/lib.rs".into()], Some(&before)).unwrap();

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
        assert!(cached(temp.path()).is_none());

        fs::write(
            &cache_path,
            r#"{"schema":"criv.source-graph/2","graph":{}}"#,
        )
        .unwrap();
        assert!(cached(temp.path()).is_none());
    }

    #[test]
    fn elixir_aliases_are_unique_or_ambiguous_and_exact_lookup_wins() {
        let owner = SymbolOwner::Module {
            name: "My.App".into(),
        };
        let mut file = SourceFile {
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
            ..SourceFile::new("lib/my_app.ex", Language::Unknown)
        };
        file.symbols.push(Symbol::new(
            "lib/my_app.ex",
            "module:My.App/fn:run/1".into(),
            SymbolKind::Function,
            "fn:literal".into(),
            SymbolRange {
                start_line: 11,
                end_line: 11,
            },
        ));
        let graph = graph_with_files([file]);

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
    fn rust_interface_ignores_function_body_changes() {
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
            before.symbols[0].interface_signature,
            after.symbols[0].interface_signature
        );
    }

    #[test]
    fn rust_interface_changes_when_function_signature_changes() {
        let before = parse_source_file("src/lib.rs", "pub fn run(input: String) -> usize { 1 }\n");
        let after = parse_source_file(
            "src/lib.rs",
            "pub fn run(input: String, verbose: bool) -> usize { 1 }\n",
        );

        assert_ne!(
            before.symbols[0].interface_signature,
            after.symbols[0].interface_signature
        );
    }

    #[test]
    fn rust_interface_tracks_struct_fields_and_enum_variants() {
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
            before_config.interface_signature.as_ref().unwrap(),
            after_config.interface_signature.as_ref().unwrap()
        );
        assert_ne!(
            before_mode.interface_signature.as_ref().unwrap(),
            after_mode.interface_signature.as_ref().unwrap()
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
        let graph = graph_with_files([file]);

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
        let graph = graph_with_files([local, other]);

        assert_eq!(
            graph.callees("src/local.rs#caller"),
            vec!["src/local.rs#fn:target"]
        );
        assert_eq!(
            graph.callers("src/local.rs#fn:target"),
            vec!["src/local.rs#fn:caller"]
        );
        assert!(graph.callers("src/other.rs#fn:target").is_empty());
    }

    #[test]
    fn an_unresolved_call_lists_every_symbol_with_its_name_as_a_callee() {
        let caller = parse_source_file("src/caller.rs", "fn caller() {\n  target();\n}\n");
        let first = parse_source_file("src/first.rs", "fn target() {}\n");
        let second = parse_source_file("src/second.rs", "fn target() {}\n");
        let graph = graph_with_files([caller, first, second]);

        assert_eq!(
            graph.callers("src/first.rs#fn:target"),
            vec!["src/caller.rs#fn:caller"]
        );
        assert_eq!(
            graph.callers("src/second.rs#fn:target"),
            vec!["src/caller.rs#fn:caller"]
        );
    }

    #[test]
    fn source_build_skips_binary_content_by_one_contract() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        let cases = vec![
            ("empty", Vec::new(), true),
            ("ascii", b"plain text\n".to_vec(), true),
            ("utf8", "hello é\n".as_bytes().to_vec(), true),
            ("invalid-utf8", vec![0x80, 0x81, b'x'], true),
            ("embedded-nul", vec![b'a', 0, b'b'], false),
            ("utf8-bom", vec![0xEF, 0xBB, 0xBF, b'x'], true),
            ("utf16-le-bom", vec![0xFF, 0xFE, b'x', 0], true),
            ("utf16-be-bom", vec![0xFE, 0xFF, 0, b'x'], true),
            ("utf32-le-bom", vec![0xFF, 0xFE, 0, 0, b'x', 0, 0, 0], true),
            ("utf32-be-bom", vec![0, 0, 0xFE, 0xFF, 0, 0, 0, b'x'], true),
            ("utf16-no-bom", vec![b'x', 0, b'y', 0], false),
            ("pdf", b"%PDF-1.7".to_vec(), false),
            ("png", vec![0x89, b'P', b'N', b'G', b'x'], false),
            ("binary-pgm", b"P5\n1 1\n255\n\xff".to_vec(), true),
            ("small-protobuf", vec![0x08, 0x96, 0x01], true),
            (
                "generated-text",
                b"(()=>{const map={version:3,sources:[]};return map})()\n".to_vec(),
                true,
            ),
            (
                "nul-at-1023",
                {
                    let mut bytes = vec![b'x'; 1024];
                    bytes[1023] = 0;
                    bytes
                },
                false,
            ),
            (
                "nul-at-1024",
                {
                    let mut bytes = vec![b'x'; 1025];
                    bytes[1024] = 0;
                    bytes
                },
                true,
            ),
        ];
        let mut paths = Vec::new();
        let mut expected = Vec::new();
        for (name, bytes, is_text) in cases {
            let path = format!("src/{name}.bin");
            fs::write(temp.path().join(&path), bytes).unwrap();
            if is_text {
                expected.push(path.clone());
            }
            paths.push(path);
        }
        expected.sort();

        assert_eq!(build(temp.path(), &paths, None).unwrap().paths(), expected);
    }

    #[test]
    fn a_file_with_a_syntax_error_keeps_its_valid_declarations() {
        let file = parse_source_file(
            "src/lib.rs",
            "pub fn valid() {\n  helper();\n}\nfn broken( {\n",
        );

        let valid = file
            .symbols
            .iter()
            .find(|symbol| symbol.name == "valid")
            .unwrap();
        assert!(valid.exported);
        assert_eq!(valid.calls[0].target, "helper");
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
        let graph = graph_with_files([file]);

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
        let built = build(root.path(), &paths, None).unwrap();
        assert_eq!(
            resolved_target(built.graph()),
            Some("lib/target.ex#module:Target/fn:run/0".into())
        );

        built.publish_at(root.path()).unwrap();
        let loaded = cached(root.path()).unwrap();
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
        let ambiguous = build(root.path(), &paths, Some(&loaded)).unwrap();
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
        let graph = graph_with_files([parse_source_file("src/lib.rs", "fn run() {}\n")]);

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
    fn typescript_interface_tracks_function_and_class_shapes() {
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

        assert_eq!(before_run.interface_signature, body_run.interface_signature);
        assert_ne!(
            before_run.interface_signature,
            changed_run.interface_signature
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

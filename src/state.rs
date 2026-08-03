use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use serde::Serialize;

use crate::c4_artifact::C4Artifact;
use crate::measurement::{self, Counter};
use crate::source_graph::{Language, SourceFile, SymbolKind};
use crate::structural;
use crate::util::write_atomic_in;
use crate::vault::{Note, NoteKind, ResolvedLink, SourceTargetResolution, Vault};
use crate::{CrivError, Result};

const STATE_SCHEMA: &str = "criv.state.v0";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct State {
    schema: &'static str,
    graph: Graph,
    #[serde(rename = "registered-patterns")]
    registered_patterns: Vec<String>,
    patterns: BTreeMap<String, Vec<PatternMatch>>,
    #[serde(rename = "source-index")]
    source_index: Vec<SourceIndexEntry>,
    #[serde(skip)]
    partitions: StatePartitions,
}

struct SerializedState {
    published: String,
    hash: String,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) partitions_rebuilt: usize,
    pub(crate) source_partitions_rebuilt: usize,
    pub(crate) note_partitions_rebuilt: usize,
    pub(crate) c4_partitions_rebuilt: usize,
    pub(crate) policy_partitions_rebuilt: usize,
    pub(crate) source_index_partitions_rebuilt: usize,
    pub(crate) serializations: usize,
    published_bytes: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        partitions_rebuilt: 0,
        source_partitions_rebuilt: 0,
        note_partitions_rebuilt: 0,
        c4_partitions_rebuilt: 0,
        policy_partitions_rebuilt: 0,
        source_index_partitions_rebuilt: 0,
        serializations: 0,
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

fn record_partition_rebuilt(kind: PartitionKind) {
    #[cfg(test)]
    record_work(|counts| {
        counts.partitions_rebuilt += 1;
        match kind {
            PartitionKind::Source => counts.source_partitions_rebuilt += 1,
            PartitionKind::Note => counts.note_partitions_rebuilt += 1,
            PartitionKind::C4Artifact => counts.c4_partitions_rebuilt += 1,
            PartitionKind::Policy => counts.policy_partitions_rebuilt += 1,
            PartitionKind::SourceIndex => counts.source_index_partitions_rebuilt += 1,
        }
    });
    #[cfg(not(test))]
    let _ = kind;
}

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct Graph {
    root: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Node {
    id: String,
    hash: String,
    kind: String,
    label: String,
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Edge {
    from: String,
    to: String,
    kind: String,
    hash: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct PatternMatch {
    file: String,
    range: Option<String>,
    captures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SourceIndexEntry {
    path: String,
    mime: Option<String>,
    frecency: u32,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
enum PartitionKey {
    Source(String),
    Note(String),
    C4Artifact(String),
    Policy(String),
    SourceIndex(String),
}

#[derive(Debug, Clone, Default)]
struct StatePartitions {
    sources: BTreeMap<String, Arc<SourcePartition>>,
    notes: BTreeMap<String, Arc<RowPartition>>,
    c4_artifacts: BTreeMap<String, Arc<RowPartition>>,
    policies: BTreeMap<String, Arc<PolicyPartition>>,
    source_index: BTreeMap<String, Arc<SourceIndexPartition>>,
    reverse_dependencies: ReverseDependencies,
    note_catalog_fingerprint: String,
}

#[derive(Debug, Clone, Default)]
struct ReverseDependencies {
    source_content: BTreeMap<String, BTreeSet<PartitionKey>>,
    call_target: BTreeMap<String, BTreeSet<PartitionKey>>,
    source_catalog: BTreeSet<PartitionKey>,
    note_catalog: BTreeSet<PartitionKey>,
    policy_catalog: BTreeSet<PartitionKey>,
}

#[derive(Debug, Clone)]
struct PartitionMeta {
    key: PartitionKey,
    input_fingerprint: String,
    dependencies: PartitionDependencies,
}

#[derive(Debug, Clone, Default)]
struct PartitionDependencies {
    source_paths: BTreeSet<String>,
    source_content_paths: BTreeSet<String>,
    call_targets: BTreeSet<String>,
    defined_symbols: BTreeSet<String>,
    catalog_sensitive: bool,
    note_catalog_sensitive: bool,
    policy_sensitive: bool,
}

#[derive(Debug, Clone)]
struct SourcePartition {
    meta: PartitionMeta,
    code_node: Node,
    rows: GraphRows,
}

#[derive(Debug, Clone)]
struct RowPartition {
    meta: PartitionMeta,
    rows: GraphRows,
}

#[derive(Debug, Clone)]
struct PolicyPartition {
    meta: PartitionMeta,
    matches: Vec<PatternMatch>,
}

#[derive(Debug, Clone)]
struct SourceIndexPartition {
    meta: PartitionMeta,
    entry: SourceIndexEntry,
}

#[derive(Debug, Clone, Default)]
struct GraphRows {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Copy)]
enum PartitionKind {
    Source,
    Note,
    C4Artifact,
    Policy,
    SourceIndex,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct C4InterfaceHashRecord {
    pub(crate) id: String,
    pub(crate) hash: String,
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) target: String,
}

impl State {
    pub(crate) fn build(root: &Path, vault: &Vault) -> Result<Self> {
        Self::build_incremental(root, vault, None, &[])
    }

    fn build_incremental(
        root: &Path,
        vault: &Vault,
        previous: Option<&State>,
        changed_files: &[String],
    ) -> Result<Self> {
        let _span = measurement::span("state.build");
        measurement::increment(Counter::StateBuilds);
        let partitions = StatePartitions::build(root, vault, previous, changed_files)?;
        Ok(Self::from_partitions(partitions))
    }

    fn from_partitions(partitions: StatePartitions) -> Self {
        let (graph, patterns, source_index) = partitions.flatten();
        Self {
            schema: STATE_SCHEMA,
            graph,
            registered_patterns: patterns.keys().cloned().collect(),
            patterns,
            source_index,
            partitions,
        }
    }

    #[cfg(test)]
    pub(crate) fn to_json(&self) -> Result<String> {
        Ok(self
            .serialize()?
            .published
            .strip_suffix('\n')
            .unwrap_or_default()
            .to_string())
    }

    #[cfg(test)]
    pub(crate) fn hash(&self) -> Result<String> {
        Ok(self.serialize()?.hash)
    }

    fn serialize(&self) -> Result<SerializedState> {
        let _span = measurement::span("state.serialize");
        measurement::increment(Counter::StateSerializations);
        let _ = &self.partitions;
        #[cfg(test)]
        record_work(|counts| counts.serializations += 1);

        let json = serde_json::to_string_pretty(self)
            .map_err(|err| CrivError::new(format!("failed to serialize state: {err}")))?;
        Ok(SerializedState {
            hash: stable_hash(&json),
            published: format!("{json}\n"),
        })
    }

    fn write_serialized(&self, root: &Path, serialized: &SerializedState) -> Result<()> {
        let _span = measurement::span("state.publish");
        write_atomic_in(
            root,
            Path::new(".criv"),
            Path::new(".criv/state.json"),
            &serialized.published,
        )?;
        measurement::increment(Counter::StatePublications);
        measurement::add(Counter::StatePublishedBytes, serialized.published.len());
        measurement::add(Counter::PublishedBytes, serialized.published.len());
        #[cfg(test)]
        record_work(|counts| counts.published_bytes += serialized.published.len());
        Ok(())
    }

    fn publish_snapshot(
        &self,
        root: &Path,
        serialized: &SerializedState,
        keep: usize,
    ) -> Result<String> {
        crate::snapshots::publish(root, &serialized.hash, &serialized.published, keep)?;
        measurement::increment(Counter::StatePublications);
        measurement::add(
            Counter::StatePublishedBytes,
            serialized.published.len() + serialized.hash.len() + 1,
        );
        measurement::add(
            Counter::PublishedBytes,
            serialized.published.len() + serialized.hash.len() + 1,
        );
        #[cfg(test)]
        record_work(|counts| {
            counts.published_bytes += serialized.published.len() + serialized.hash.len() + 1;
        });
        Ok(serialized.hash.clone())
    }
}

#[derive(Debug, Default)]
struct InvalidationFacts {
    changed_source_paths: BTreeSet<String>,
    invalidated_partitions: BTreeSet<PartitionKey>,
}

impl InvalidationFacts {
    fn collect(
        vault: &Vault,
        previous: Option<&StatePartitions>,
        changed_files: &[String],
        policy_fingerprints: &BTreeMap<String, String>,
        note_catalog_fingerprint: &str,
    ) -> Self {
        let Some(previous) = previous else {
            return Self {
                changed_source_paths: changed_files.iter().cloned().collect(),
                ..Self::default()
            };
        };

        let current_paths = vault
            .source_graph()
            .files
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let previous_paths = previous.sources.keys().cloned().collect::<BTreeSet<_>>();
        let mut changed_source_paths = changed_files.iter().cloned().collect::<BTreeSet<_>>();
        changed_source_paths.extend(current_paths.symmetric_difference(&previous_paths).cloned());

        let mut changed_symbol_names = BTreeSet::new();
        for path in &changed_source_paths {
            if let Some(file) = vault.source_graph().files.get(path) {
                changed_symbol_names.extend(file.symbols.iter().map(|symbol| symbol.name.clone()));
            }
            if let Some(partition) = previous.sources.get(path) {
                changed_symbol_names
                    .extend(partition.meta.dependencies.defined_symbols.iter().cloned());
            }
        }

        let policy_changed = policy_fingerprints.len() != previous.policies.len()
            || policy_fingerprints.iter().any(|(id, fingerprint)| {
                previous
                    .policies
                    .get(id)
                    .is_none_or(|partition| &partition.meta.input_fingerprint != fingerprint)
            });

        let mut invalidated_partitions = BTreeSet::new();
        for path in &changed_source_paths {
            if let Some(dependents) = previous.reverse_dependencies.source_content.get(path) {
                invalidated_partitions.extend(dependents.iter().cloned());
            }
        }
        for name in &changed_symbol_names {
            if let Some(dependents) = previous.reverse_dependencies.call_target.get(name) {
                invalidated_partitions.extend(dependents.iter().cloned());
            }
        }
        if current_paths != previous_paths {
            invalidated_partitions
                .extend(previous.reverse_dependencies.source_catalog.iter().cloned());
        }
        if note_catalog_fingerprint != previous.note_catalog_fingerprint {
            invalidated_partitions
                .extend(previous.reverse_dependencies.note_catalog.iter().cloned());
        }
        if policy_changed {
            invalidated_partitions
                .extend(previous.reverse_dependencies.policy_catalog.iter().cloned());
        }

        Self {
            changed_source_paths,
            invalidated_partitions,
        }
    }

    fn affects(&self, key: &PartitionKey) -> bool {
        self.invalidated_partitions.contains(key)
    }
}

impl ReverseDependencies {
    fn index(partitions: &StatePartitions) -> Self {
        let mut index = Self::default();
        for (path, partition) in &partitions.sources {
            index.insert(
                PartitionKey::Source(path.clone()),
                &partition.meta.dependencies,
            );
        }
        for (path, partition) in &partitions.notes {
            index.insert(
                PartitionKey::Note(path.clone()),
                &partition.meta.dependencies,
            );
        }
        for (path, partition) in &partitions.c4_artifacts {
            index.insert(
                PartitionKey::C4Artifact(path.clone()),
                &partition.meta.dependencies,
            );
        }
        for (id, partition) in &partitions.policies {
            index.insert(
                PartitionKey::Policy(id.clone()),
                &partition.meta.dependencies,
            );
        }
        index
    }

    fn insert(&mut self, key: PartitionKey, dependencies: &PartitionDependencies) {
        for path in &dependencies.source_content_paths {
            self.source_content
                .entry(path.clone())
                .or_default()
                .insert(key.clone());
        }
        for target in &dependencies.call_targets {
            self.call_target
                .entry(target.clone())
                .or_default()
                .insert(key.clone());
        }
        if dependencies.catalog_sensitive {
            self.source_catalog.insert(key.clone());
        }
        if dependencies.note_catalog_sensitive {
            self.note_catalog.insert(key.clone());
        }
        if dependencies.policy_sensitive {
            self.policy_catalog.insert(key);
        }
    }
}

impl StatePartitions {
    fn build(
        root: &Path,
        vault: &Vault,
        previous: Option<&State>,
        changed_files: &[String],
    ) -> Result<Self> {
        let previous = previous.map(|state| &state.partitions);
        let policy_fingerprints = policy_fingerprints(vault);
        let note_catalog_fingerprint = note_catalog_fingerprint(vault);
        let invalidation = InvalidationFacts::collect(
            vault,
            previous,
            changed_files,
            &policy_fingerprints,
            &note_catalog_fingerprint,
        );
        let mut partitions = Self::default();

        for file in vault.source_graph().files.values() {
            let key = PartitionKey::Source(file.path.clone());
            let partition = previous
                .and_then(|previous| previous.sources.get(&file.path))
                .filter(|_| {
                    !invalidation.changed_source_paths.contains(&file.path)
                        && !invalidation.affects(&key)
                })
                .cloned()
                .unwrap_or_else(|| {
                    record_partition_rebuilt(PartitionKind::Source);
                    Arc::new(build_source_partition(vault, file))
                });
            partitions.sources.insert(file.path.clone(), partition);
        }

        for note in &vault.notes {
            let key = PartitionKey::Note(note.rel_path.clone());
            let fingerprint = note_input_fingerprint(vault, note);
            let partition = previous
                .and_then(|previous| previous.notes.get(&note.rel_path))
                .filter(|partition| {
                    partition.meta.input_fingerprint == fingerprint && !invalidation.affects(&key)
                })
                .cloned()
                .unwrap_or_else(|| {
                    record_partition_rebuilt(PartitionKind::Note);
                    Arc::new(build_note_partition(vault, note))
                });
            partitions.notes.insert(note.rel_path.clone(), partition);
        }

        for artifact in &vault.c4_artifacts {
            let key = PartitionKey::C4Artifact(artifact.rel_path.clone());
            let fingerprint = c4_artifact_input_fingerprint(artifact);
            let partition = previous
                .and_then(|previous| previous.c4_artifacts.get(&artifact.rel_path))
                .filter(|partition| {
                    partition.meta.input_fingerprint == fingerprint && !invalidation.affects(&key)
                })
                .cloned()
                .unwrap_or_else(|| {
                    record_partition_rebuilt(PartitionKind::C4Artifact);
                    Arc::new(build_c4_artifact_partition(vault, artifact))
                });
            partitions
                .c4_artifacts
                .insert(artifact.rel_path.clone(), partition);
        }

        let mut source_entries = vault.source_index().entries()?;
        source_entries.sort_by(|left, right| left.path.cmp(&right.path));
        for entry in source_entries {
            let state_entry = SourceIndexEntry {
                mime: mime_guess::from_path(&entry.path)
                    .first_raw()
                    .map(str::to_string),
                path: entry.path.clone(),
                frecency: entry.frecency,
            };
            let fingerprint = source_index_input_fingerprint(&state_entry);
            let partition = previous
                .and_then(|previous| previous.source_index.get(&entry.path))
                .filter(|partition| partition.meta.input_fingerprint == fingerprint)
                .cloned()
                .unwrap_or_else(|| {
                    record_partition_rebuilt(PartitionKind::SourceIndex);
                    Arc::new(SourceIndexPartition {
                        meta: PartitionMeta {
                            key: PartitionKey::SourceIndex(entry.path.clone()),
                            input_fingerprint: fingerprint,
                            dependencies: PartitionDependencies::default(),
                        },
                        entry: state_entry,
                    })
                });
            partitions.source_index.insert(entry.path, partition);
        }

        partitions.policies =
            build_policy_partitions(root, vault, previous, changed_files, &policy_fingerprints)?;
        partitions.reverse_dependencies = ReverseDependencies::index(&partitions);
        partitions.note_catalog_fingerprint = note_catalog_fingerprint;
        Ok(partitions)
    }

    fn flatten(
        &self,
    ) -> (
        Graph,
        BTreeMap<String, Vec<PatternMatch>>,
        Vec<SourceIndexEntry>,
    ) {
        let mut graph = Graph::default();
        let mut seen_nodes = BTreeSet::new();
        let mut seen_edges = BTreeSet::new();

        // Keep the public v0 ordering: every code file node precedes source details.
        for (path, partition) in &self.sources {
            observe_partition_meta(&partition.meta, &PartitionKey::Source(path.clone()));
            add_node(&mut graph, &mut seen_nodes, partition.code_node.clone());
        }
        for partition in self.sources.values() {
            append_graph_rows(
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                &partition.rows,
            );
        }
        for (path, partition) in &self.notes {
            observe_partition_meta(&partition.meta, &PartitionKey::Note(path.clone()));
            append_graph_rows(
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                &partition.rows,
            );
        }
        for (path, partition) in &self.c4_artifacts {
            observe_partition_meta(&partition.meta, &PartitionKey::C4Artifact(path.clone()));
            append_graph_rows(
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                &partition.rows,
            );
        }
        graph.root = graph_root(&graph);

        let patterns = self
            .policies
            .iter()
            .map(|(id, partition)| {
                observe_partition_meta(&partition.meta, &PartitionKey::Policy(id.clone()));
                (id.clone(), partition.matches.clone())
            })
            .collect();
        let source_index = self
            .source_index
            .iter()
            .map(|(path, partition)| {
                observe_partition_meta(&partition.meta, &PartitionKey::SourceIndex(path.clone()));
                partition.entry.clone()
            })
            .collect();

        (graph, patterns, source_index)
    }
}

fn partition_meta(
    key: PartitionKey,
    input_fingerprint: String,
    dependencies: PartitionDependencies,
) -> PartitionMeta {
    PartitionMeta {
        key,
        input_fingerprint,
        dependencies,
    }
}

fn source_input_fingerprint(file: &SourceFile) -> String {
    let mut hasher = blake3::Hasher::new();
    fingerprint_str(&mut hasher, &file.path);
    fingerprint_str(&mut hasher, language_name(file.language));
    for import in &file.imports {
        fingerprint_str(&mut hasher, &import.module);
        fingerprint_usize(&mut hasher, import.line);
    }
    for symbol in &file.symbols {
        fingerprint_str(&mut hasher, &symbol.id.display());
        fingerprint_str(&mut hasher, &symbol.name);
        fingerprint_str(&mut hasher, symbol_kind(symbol.kind));
        fingerprint_option_str(&mut hasher, symbol.parent.as_deref());
        fingerprint_usize(&mut hasher, symbol.range.start_line);
        fingerprint_usize(&mut hasher, symbol.range.end_line);
        for call in &symbol.calls {
            fingerprint_str(&mut hasher, &call.target);
            fingerprint_usize(&mut hasher, call.line);
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn note_input_fingerprint(vault: &Vault, note: &Note) -> String {
    let mut hasher = blake3::Hasher::new();
    fingerprint_str(&mut hasher, &note.rel_path);
    fingerprint_option_str(&mut hasher, note.id.as_deref());
    fingerprint_str(
        &mut hasher,
        match note.kind {
            NoteKind::Decision => "decision",
            NoteKind::Doc => "doc",
            NoteKind::Unknown => "unknown",
        },
    );
    fingerprint_option_str(&mut hasher, note.title.as_deref());
    fingerprint_option_str(&mut hasher, note.status.as_deref());
    fingerprint_str(&mut hasher, &note.body);
    fingerprint_str(&mut hasher, &format!("{:?}", note.targets_symbols));
    fingerprint_str(&mut hasher, &format!("{:?}", note.targets_scope));
    fingerprint_str(&mut hasher, &format!("{:?}", note.target_pattern_refs));
    fingerprint_str(&mut hasher, &format!("{:?}", note.target_pattern_ids));
    fingerprint_str(&mut hasher, &format!("{:?}", note.policy_patterns));
    fingerprint_str(&mut hasher, &format!("{:?}", note.governs));
    fingerprint_str(&mut hasher, &format!("{:?}", note.supersedes));
    fingerprint_str(&mut hasher, &format!("{:?}", note.superseded_by));
    fingerprint_str(&mut hasher, &format!("{:?}", note.frontmatter_error));
    fingerprint_str(&mut hasher, &format!("{:?}", vault.effective_governs(note)));
    hasher.finalize().to_hex().to_string()
}

fn note_catalog_fingerprint(vault: &Vault) -> String {
    let mut hasher = blake3::Hasher::new();
    for note in &vault.notes {
        fingerprint_str(&mut hasher, &note.rel_path);
        fingerprint_option_str(&mut hasher, note.id.as_deref());
        fingerprint_option_str(&mut hasher, note.title.as_deref());
        for heading in &note.headings {
            fingerprint_str(&mut hasher, &heading.text);
            fingerprint_usize(&mut hasher, heading.level);
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn c4_artifact_input_fingerprint(artifact: &C4Artifact) -> String {
    stable_hash(&format!("{artifact:#?}"))
}

fn source_index_input_fingerprint(entry: &SourceIndexEntry) -> String {
    stable_hash(&format!(
        "{}\0{}\0{:?}",
        entry.path, entry.frecency, entry.mime
    ))
}

fn fingerprint_str(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn fingerprint_option_str(hasher: &mut blake3::Hasher, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update(&[1]);
            fingerprint_str(hasher, value);
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

fn fingerprint_usize(hasher: &mut blake3::Hasher, value: usize) {
    hasher.update(&(value as u64).to_le_bytes());
}

fn observe_partition_meta(meta: &PartitionMeta, expected_key: &PartitionKey) {
    debug_assert_eq!(&meta.key, expected_key);
    let _ = (
        &meta.input_fingerprint,
        &meta.dependencies.source_paths,
        &meta.dependencies.source_content_paths,
        &meta.dependencies.call_targets,
        &meta.dependencies.defined_symbols,
        meta.dependencies.catalog_sensitive,
        meta.dependencies.note_catalog_sensitive,
        meta.dependencies.policy_sensitive,
    );
}

fn append_graph_rows(
    graph: &mut Graph,
    seen_nodes: &mut BTreeSet<String>,
    seen_edges: &mut BTreeSet<String>,
    rows: &GraphRows,
) {
    for node in &rows.nodes {
        add_node(graph, seen_nodes, node.clone());
    }
    for edge in &rows.edges {
        add_edge(graph, seen_edges, &edge.from, &edge.to, &edge.kind);
    }
}

fn graph_rows(graph: Graph) -> GraphRows {
    GraphRows {
        nodes: graph.nodes,
        edges: graph.edges,
    }
}

fn build_source_partition(vault: &Vault, file: &SourceFile) -> SourcePartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();
    let mut dependencies = PartitionDependencies::default();
    dependencies.source_paths.insert(file.path.clone());
    let file_id = code_node_id(&file.path);

    for import in &file.imports {
        let import_id = import_node_id(&file.path, &import.module);
        add_node(
            &mut graph,
            &mut seen_nodes,
            Node {
                id: import_id.clone(),
                hash: String::new(),
                kind: "import".into(),
                label: import.module.clone(),
                path: Some(format!("{}#L{}", file.path, import.line)),
            },
        );
        add_edge(&mut graph, &mut seen_edges, &file_id, &import_id, "imports");
    }

    for symbol in &file.symbols {
        dependencies.defined_symbols.insert(symbol.name.clone());
        let symbol_id = symbol_node_id(&symbol.id.display());
        add_node(
            &mut graph,
            &mut seen_nodes,
            Node {
                id: symbol_id.clone(),
                hash: String::new(),
                kind: symbol_kind(symbol.kind).into(),
                label: symbol.name.clone(),
                path: Some(format!(
                    "{}#L{}-L{}",
                    symbol.id.path, symbol.range.start_line, symbol.range.end_line
                )),
            },
        );
        add_edge(
            &mut graph,
            &mut seen_edges,
            &file_id,
            &symbol_id,
            "contains",
        );
        if let Some(parent) = &symbol.parent
            && let Some(parent_id) = vault
                .source_graph()
                .resolve_symbol(&format!("{}#{}", symbol.id.path, parent))
        {
            dependencies.source_paths.insert(parent_id.path.clone());
            add_edge(
                &mut graph,
                &mut seen_edges,
                &symbol_node_id(&parent_id.display()),
                &symbol_id,
                "contains",
            );
        }
        for call in &symbol.calls {
            dependencies.call_targets.insert(call.target.clone());
            let resolved = vault.source_graph().resolve_call(&symbol.id, &call.target);
            if let Some(target) = &resolved {
                dependencies.source_paths.insert(target.path.clone());
            } else {
                dependencies.catalog_sensitive = true;
            }
            let target = resolved
                .map(|target| symbol_node_id(&target.display()))
                .unwrap_or_else(|| external_call_node_id(&call.target));
            if target.starts_with("external-call:") {
                add_node(
                    &mut graph,
                    &mut seen_nodes,
                    Node {
                        id: target.clone(),
                        hash: String::new(),
                        kind: "external-call".into(),
                        label: call.target.clone(),
                        path: Some(format!("{}#L{}", symbol.id.path, call.line)),
                    },
                );
            }
            add_edge(&mut graph, &mut seen_edges, &symbol_id, &target, "calls");
        }
    }

    SourcePartition {
        meta: partition_meta(
            PartitionKey::Source(file.path.clone()),
            source_input_fingerprint(file),
            dependencies,
        ),
        code_node: Node {
            id: file_id,
            hash: String::new(),
            kind: "code".into(),
            label: format!("{} ({})", file.path, language_name(file.language)),
            path: Some(file.path.clone()),
        },
        rows: graph_rows(graph),
    }
}

fn build_note_partition(vault: &Vault, note: &Note) -> RowPartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();
    let mut dependencies = PartitionDependencies {
        note_catalog_sensitive: !note.wiki_links.is_empty(),
        policy_sensitive: note.kind == NoteKind::Decision,
        ..PartitionDependencies::default()
    };
    let kind = match note.kind {
        NoteKind::Decision => "decision",
        NoteKind::Doc | NoteKind::Unknown => "doc",
    };
    let note_id = note_node_id(note.display_id());
    add_node(
        &mut graph,
        &mut seen_nodes,
        Node {
            id: note_id.clone(),
            hash: String::new(),
            kind: kind.into(),
            label: note
                .title
                .clone()
                .unwrap_or_else(|| note.display_id().to_string()),
            path: Some(note.rel_path.clone()),
        },
    );

    for target in &note.targets_symbols {
        dependencies.catalog_sensitive = true;
        match vault.resolve_source_target(target) {
            SourceTargetResolution::Resolved { path, .. } => {
                dependencies.source_paths.insert(path.clone());
                dependencies.source_content_paths.insert(path.clone());
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &code_node_id(&path),
                    "references",
                );
            }
            SourceTargetResolution::MissingFragment { path } => {
                dependencies.source_paths.insert(path.clone());
                dependencies.source_content_paths.insert(path);
            }
            SourceTargetResolution::MissingFile => {}
        }
    }

    for heading in &note.headings {
        let heading_id = format!("{note_id}#{}", crate::util::kebab(&heading.text));
        add_node(
            &mut graph,
            &mut seen_nodes,
            Node {
                id: heading_id.clone(),
                hash: String::new(),
                kind: "doc-heading".into(),
                label: heading.text.clone(),
                path: Some(format!(
                    "{}#L{}:H{}",
                    note.rel_path, heading.line, heading.level
                )),
            },
        );
        add_edge(
            &mut graph,
            &mut seen_edges,
            &note_id,
            &heading_id,
            "contains",
        );
    }

    let governs = vault.effective_governs(note);
    if !governs.is_empty() {
        dependencies.catalog_sensitive = true;
    }
    for source_file in vault.source_files_matching_globs(&governs) {
        dependencies.source_paths.insert(source_file.clone());
        add_edge(
            &mut graph,
            &mut seen_edges,
            &note_id,
            &code_node_id(&source_file),
            "governs",
        );
    }

    for superseded in &note.supersedes {
        add_edge(
            &mut graph,
            &mut seen_edges,
            &note_id,
            &note_node_id(superseded),
            "supersedes",
        );
    }

    for link in &note.wiki_links {
        match vault.resolve_link(&link.target) {
            ResolvedLink::Note { id } => {
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &note_node_id(&id),
                    "cites",
                );
            }
            ResolvedLink::Source { path, .. } => {
                dependencies.catalog_sensitive = true;
                dependencies.source_paths.insert(path.clone());
                if crate::vault::source_fragment_name(&link.target).is_some() {
                    dependencies.source_content_paths.insert(path.clone());
                }
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &code_node_id(&path),
                    "references",
                );
            }
            ResolvedLink::Pattern { id } => {
                dependencies.policy_sensitive = true;
                let pattern_id = pattern_node_id(&id);
                add_node(
                    &mut graph,
                    &mut seen_nodes,
                    Node {
                        id: pattern_id.clone(),
                        hash: String::new(),
                        kind: "pattern".into(),
                        label: id,
                        path: None,
                    },
                );
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &pattern_id,
                    "references",
                );
            }
            ResolvedLink::Broken => {
                dependencies.catalog_sensitive = true;
                if let SourceTargetResolution::MissingFragment { path } =
                    vault.resolve_source_target(&link.target)
                {
                    dependencies.source_paths.insert(path.clone());
                    dependencies.source_content_paths.insert(path);
                }
            }
        }
    }

    if !note.c4_diagrams.is_empty() {
        dependencies.catalog_sensitive = true;
    }
    add_c4_diagrams_to_graph(
        &mut graph,
        &mut seen_nodes,
        &mut seen_edges,
        vault,
        &note_id,
        &note.rel_path,
        &note.c4_diagrams,
    );
    collect_c4_source_dependencies(vault, &note.c4_diagrams, &mut dependencies);
    collect_graph_source_dependencies(&graph, &mut dependencies);

    RowPartition {
        meta: partition_meta(
            PartitionKey::Note(note.rel_path.clone()),
            note_input_fingerprint(vault, note),
            dependencies,
        ),
        rows: graph_rows(graph),
    }
}

fn build_c4_artifact_partition(vault: &Vault, artifact: &C4Artifact) -> RowPartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();
    let artifact_id = c4_artifact_node_id(&artifact.rel_path);
    add_node(
        &mut graph,
        &mut seen_nodes,
        Node {
            id: artifact_id.clone(),
            hash: String::new(),
            kind: "c4-artifact".into(),
            label: artifact.rel_path.clone(),
            path: Some(artifact.rel_path.clone()),
        },
    );
    add_c4_diagrams_to_graph(
        &mut graph,
        &mut seen_nodes,
        &mut seen_edges,
        vault,
        &artifact_id,
        &artifact.rel_path,
        &artifact.diagrams,
    );
    let mut dependencies = PartitionDependencies {
        catalog_sensitive: !artifact.diagrams.is_empty(),
        ..PartitionDependencies::default()
    };
    collect_c4_source_dependencies(vault, &artifact.diagrams, &mut dependencies);
    collect_graph_source_dependencies(&graph, &mut dependencies);

    RowPartition {
        meta: partition_meta(
            PartitionKey::C4Artifact(artifact.rel_path.clone()),
            c4_artifact_input_fingerprint(artifact),
            dependencies,
        ),
        rows: graph_rows(graph),
    }
}

fn collect_c4_source_dependencies(
    vault: &Vault,
    diagrams: &[crate::c4::C4Diagram],
    dependencies: &mut PartitionDependencies,
) {
    for source in diagrams
        .iter()
        .flat_map(|diagram| &diagram.elements)
        .filter_map(|element| element.source.as_deref())
    {
        dependencies.catalog_sensitive = true;
        match vault.resolve_source_target(source) {
            SourceTargetResolution::Resolved { path, .. }
            | SourceTargetResolution::MissingFragment { path } => {
                dependencies.source_paths.insert(path.clone());
                if crate::vault::source_fragment_name(source).is_some() {
                    dependencies.source_content_paths.insert(path);
                }
            }
            SourceTargetResolution::MissingFile => {}
        }
    }
}

fn collect_graph_source_dependencies(graph: &Graph, dependencies: &mut PartitionDependencies) {
    for edge in &graph.edges {
        if let Some(path) = edge.to.strip_prefix("code:") {
            dependencies.source_paths.insert(path.to_string());
        }
    }
    for node in &graph.nodes {
        if node.kind == "c4-interface"
            && let Some(path) = node
                .path
                .as_deref()
                .and_then(|target| target.split('#').next())
        {
            dependencies.source_content_paths.insert(path.to_string());
        }
    }
}

fn build_policy_partitions(
    root: &Path,
    vault: &Vault,
    previous: Option<&StatePartitions>,
    changed_files: &[String],
    fingerprints: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Arc<PolicyPartition>>> {
    let mut partitions = BTreeMap::new();
    let mut pending_policy_scans = Vec::new();
    for pattern_id in vault.patterns() {
        if let Some((note, policy)) = vault.resolve_policy_pattern(pattern_id) {
            let scopes = vault.effective_governs(note);
            let input_fingerprint = fingerprints
                .get(pattern_id)
                .expect("every registered pattern has an input fingerprint")
                .clone();
            let previous_partition =
                previous.and_then(|previous| previous.policies.get(pattern_id));
            let definition_unchanged = previous_partition
                .is_some_and(|partition| partition.meta.input_fingerprint == input_fingerprint);
            let paths = if definition_unchanged {
                changed_paths_in_scopes(changed_files, &scopes)
            } else {
                vault.source_files_matching_globs(&scopes)
            }
            .into_iter()
            .collect::<BTreeSet<_>>();

            if definition_unchanged && paths.is_empty() {
                partitions.insert(
                    pattern_id.clone(),
                    previous_partition
                        .expect("unchanged definitions have a previous partition")
                        .clone(),
                );
                continue;
            }

            let reused = if definition_unchanged {
                previous_partition.map_or_else(Vec::new, |partition| {
                    reusable_matches(&partition.matches, &paths)
                })
            } else {
                Vec::new()
            };
            pending_policy_scans.push(PendingPolicyScan {
                pattern_id: pattern_id.clone(),
                input_fingerprint,
                policy,
                paths,
                reused,
            });
        }
    }

    let compiled_requests = pending_policy_scans
        .iter()
        .enumerate()
        .map(|(key, scan)| {
            structural::compile_policy(scan.policy)
                .map(|policy| (key, policy))
                .map_err(CrivError::from)
        })
        .collect::<Result<Vec<_>>>()?;
    let requests = compiled_requests
        .iter()
        .map(|(key, policy)| structural::PolicyScanRequest {
            key: *key,
            policy,
            paths: &pending_policy_scans[*key].paths,
        })
        .collect::<Vec<_>>();
    let rescanned = structural::find_policies_batch(root, vault, &requests)?;
    for (key, scan) in pending_policy_scans.into_iter().enumerate() {
        let mut matches = scan.reused;
        matches.extend(
            rescanned
                .get(&key)
                .expect("every policy scan request has a result")
                .iter()
                .map(pattern_match_from_structural),
        );
        sort_and_dedup_pattern_matches(&mut matches);
        record_partition_rebuilt(PartitionKind::Policy);
        let pattern_id = scan.pattern_id;
        partitions.insert(
            pattern_id.clone(),
            Arc::new(PolicyPartition {
                meta: partition_meta(
                    PartitionKey::Policy(pattern_id),
                    scan.input_fingerprint,
                    PartitionDependencies {
                        catalog_sensitive: true,
                        policy_sensitive: true,
                        ..PartitionDependencies::default()
                    },
                ),
                matches,
            }),
        );
    }
    Ok(partitions)
}

fn policy_fingerprints(vault: &Vault) -> BTreeMap<String, String> {
    vault
        .patterns()
        .iter()
        .filter_map(|pattern_id| {
            vault
                .resolve_policy_pattern(pattern_id)
                .map(|(note, policy)| {
                    (
                        pattern_id.clone(),
                        stable_hash(&format!("{policy:#?}\0{:?}", vault.effective_governs(note))),
                    )
                })
        })
        .collect()
}

pub(crate) fn write_state(root: &Path, vault: &Vault) -> Result<(String, State)> {
    let state = State::build(root, vault)?;
    let serialized = state.serialize()?;
    state.write_serialized(root, &serialized)?;
    let snapshot = state.publish_snapshot(root, &serialized, vault.config.state_keep)?;
    Ok((snapshot, state))
}

pub(crate) fn write_state_incremental(
    root: &Path,
    vault: &Vault,
    previous: Option<&State>,
    changed_files: &[String],
) -> Result<(String, State)> {
    let state = State::build_incremental(root, vault, previous, changed_files)?;
    let serialized = state.serialize()?;
    state.write_serialized(root, &serialized)?;
    let snapshot = state.publish_snapshot(root, &serialized, vault.config.state_keep)?;
    Ok((snapshot, state))
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

struct PendingPolicyScan<'a> {
    pattern_id: String,
    input_fingerprint: String,
    policy: &'a crate::vault::PolicyPattern,
    paths: BTreeSet<String>,
    reused: Vec<PatternMatch>,
}

fn reusable_matches(
    previous_matches: &[PatternMatch],
    rescanned_paths: &BTreeSet<String>,
) -> Vec<PatternMatch> {
    previous_matches
        .iter()
        .filter(|matched| !rescanned_paths.contains(&matched.file))
        .cloned()
        .collect()
}

fn pattern_match_from_structural(matched: &structural::StructuralMatch) -> PatternMatch {
    PatternMatch {
        file: matched.path.clone(),
        range: Some(matched.range.clone()),
        captures: matched.captures.clone(),
    }
}

fn sort_and_dedup_pattern_matches(matches: &mut Vec<PatternMatch>) {
    matches.sort_by(|left, right| {
        (&left.file, &left.range, &left.captures).cmp(&(&right.file, &right.range, &right.captures))
    });
    matches.dedup();
}

fn changed_paths_in_scopes(paths: &[String], scopes: &[String]) -> Vec<String> {
    let matcher = crate::util::GlobMatcher::from_valid_patterns(scopes);
    paths
        .iter()
        .filter(|path| matcher.is_match(path))
        .cloned()
        .collect()
}

fn add_c4_diagrams_to_graph(
    graph: &mut Graph,
    seen_nodes: &mut BTreeSet<String>,
    seen_edges: &mut BTreeSet<String>,
    vault: &Vault,
    owner_id: &str,
    owner_path: &str,
    diagrams: &[crate::c4::C4Diagram],
) {
    for diagram in diagrams {
        let diagram_id = c4_diagram_node_id(owner_id, diagram.line);
        add_node(
            graph,
            seen_nodes,
            Node {
                id: diagram_id.clone(),
                hash: String::new(),
                kind: "c4-diagram".into(),
                label: format!("{} diagram", diagram.level.as_str()),
                path: Some(format!("{owner_path}#L{}", diagram.line)),
            },
        );
        add_edge(graph, seen_edges, owner_id, &diagram_id, "contains");

        let mut element_nodes = BTreeMap::new();
        for element in &diagram.elements {
            let element_id = c4_element_node_id(owner_id, diagram.line, &element.alias);
            element_nodes
                .entry(element.alias.as_str())
                .or_insert_with(|| element_id.clone());
            add_node(
                graph,
                seen_nodes,
                Node {
                    id: element_id.clone(),
                    hash: String::new(),
                    kind: format!("c4-{}", element.category.as_str()),
                    label: if element.label.is_empty() {
                        element.alias.clone()
                    } else {
                        element.label.clone()
                    },
                    path: Some(format!("{owner_path}#L{}", element.line)),
                },
            );
            add_edge(graph, seen_edges, owner_id, &element_id, "contains");
            add_edge(graph, seen_edges, &diagram_id, &element_id, "contains");
            if let Some(source) = &element.source
                && let SourceTargetResolution::Resolved { path, .. } =
                    vault.resolve_source_target(source)
            {
                add_edge(
                    graph,
                    seen_edges,
                    &element_id,
                    &code_node_id(&path),
                    "references",
                );
                if let Some((target, interface_hash)) = interface_anchor_hash(vault, source, &path)
                {
                    let interface_id = c4_interface_node_id(&element_id);
                    add_node(
                        graph,
                        seen_nodes,
                        Node {
                            id: interface_id.clone(),
                            hash: String::new(),
                            kind: "c4-interface".into(),
                            label: interface_hash,
                            path: Some(target),
                        },
                    );
                    add_edge(
                        graph,
                        seen_edges,
                        &element_id,
                        &interface_id,
                        "tracks-interface",
                    );
                }
            }
        }

        for relationship in &diagram.relationships {
            if let (Some(from), Some(to)) = (
                element_nodes.get(relationship.from.as_str()),
                element_nodes.get(relationship.to.as_str()),
            ) {
                let relationship_id = c4_relationship_node_id(
                    owner_id,
                    diagram.line,
                    relationship.line,
                    &relationship.from,
                    &relationship.to,
                );
                add_node(
                    graph,
                    seen_nodes,
                    Node {
                        id: relationship_id.clone(),
                        hash: String::new(),
                        kind: "c4-relationship".into(),
                        label: relationship.label.clone().unwrap_or_else(|| {
                            format!("{} -> {}", relationship.from, relationship.to)
                        }),
                        path: Some(format!("{owner_path}#L{}", relationship.line)),
                    },
                );
                add_edge(graph, seen_edges, owner_id, &relationship_id, "contains");
                add_edge(graph, seen_edges, &diagram_id, &relationship_id, "contains");
                add_edge(graph, seen_edges, &relationship_id, from, "from");
                add_edge(graph, seen_edges, &relationship_id, to, "to");
                add_edge(graph, seen_edges, from, to, "relates");
            }
        }
    }
}

pub(crate) fn c4_interface_hash_records(vault: &Vault) -> Vec<C4InterfaceHashRecord> {
    let mut records = Vec::new();
    for note in &vault.notes {
        let owner_id = note_node_id(note.display_id());
        collect_c4_interface_hashes(
            vault,
            &owner_id,
            &note.rel_path,
            &note.c4_diagrams,
            &mut records,
        );
    }
    for artifact in &vault.c4_artifacts {
        let owner_id = c4_artifact_node_id(&artifact.rel_path);
        collect_c4_interface_hashes(
            vault,
            &owner_id,
            &artifact.rel_path,
            &artifact.diagrams,
            &mut records,
        );
    }
    records
}

fn collect_c4_interface_hashes(
    vault: &Vault,
    owner_id: &str,
    owner_path: &str,
    diagrams: &[crate::c4::C4Diagram],
    records: &mut Vec<C4InterfaceHashRecord>,
) {
    for diagram in diagrams {
        for element in &diagram.elements {
            let Some(source) = &element.source else {
                continue;
            };
            let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(source)
            else {
                continue;
            };
            let Some((target, interface_hash)) = interface_anchor_hash(vault, source, &path) else {
                continue;
            };
            let element_id = c4_element_node_id(owner_id, diagram.line, &element.alias);
            records.push(C4InterfaceHashRecord {
                id: c4_interface_node_id(&element_id),
                hash: interface_hash,
                path: owner_path.to_string(),
                line: element.line,
                target,
            });
        }
    }
}

impl State {
    pub(crate) fn c4_interface_hashes(&self) -> BTreeMap<String, String> {
        self.graph
            .nodes
            .iter()
            .filter(|node| node.kind == "c4-interface")
            .map(|node| (node.id.clone(), node.label.clone()))
            .collect()
    }
}

fn interface_anchor_hash(vault: &Vault, source: &str, path: &str) -> Option<(String, String)> {
    let fragment = crate::vault::source_fragment_name(source)?;
    let target = format!("{path}#{fragment}");
    let hash = vault.source_graph().interface_hash(&target)?;
    Some((target, hash))
}

fn add_node(graph: &mut Graph, seen: &mut BTreeSet<String>, node: Node) {
    let mut node = node;
    node.hash = node_hash(&node);
    if seen.insert(node.id.clone()) {
        graph.nodes.push(node);
    }
}

fn add_edge(graph: &mut Graph, seen: &mut BTreeSet<String>, from: &str, to: &str, kind: &str) {
    let key = format!("{from}\0{to}\0{kind}");
    if seen.insert(key) {
        let mut edge = Edge {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
            hash: String::new(),
        };
        edge.hash = edge_hash(&edge);
        graph.edges.push(edge);
    }
}

fn node_hash(node: &Node) -> String {
    stable_hash(&format!(
        "node\0{}\0{}\0{}\0{}",
        node.id,
        node.kind,
        node.label,
        node.path.as_deref().unwrap_or("")
    ))
}

fn edge_hash(edge: &Edge) -> String {
    stable_hash(&format!("edge\0{}\0{}\0{}", edge.from, edge.kind, edge.to))
}

fn graph_root(graph: &Graph) -> String {
    let mut hashes = graph
        .nodes
        .iter()
        .map(|node| node.hash.as_str())
        .chain(graph.edges.iter().map(|edge| edge.hash.as_str()))
        .collect::<Vec<_>>();
    hashes.sort();
    stable_hash(&hashes.join("\n"))
}

fn stable_hash(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}

fn note_node_id(id: &str) -> String {
    format!("note:{id}")
}

fn code_node_id(path: &str) -> String {
    format!("code:{path}")
}

fn pattern_node_id(id: &str) -> String {
    format!("pattern:{id}")
}

fn import_node_id(path: &str, module: &str) -> String {
    format!("import:{path}:{module}")
}

fn symbol_node_id(id: &str) -> String {
    format!("symbol:{id}")
}

fn external_call_node_id(id: &str) -> String {
    format!("external-call:{id}")
}

fn c4_artifact_node_id(path: &str) -> String {
    format!("c4-artifact:{path}")
}

fn c4_diagram_node_id(owner_id: &str, diagram_line: usize) -> String {
    format!("{owner_id}:c4:{diagram_line}")
}

fn c4_element_node_id(owner_id: &str, diagram_line: usize, alias: &str) -> String {
    format!("{owner_id}:c4:{diagram_line}:{alias}")
}

fn c4_interface_node_id(element_id: &str) -> String {
    format!("{element_id}:interface")
}

fn c4_relationship_node_id(
    owner_id: &str,
    diagram_line: usize,
    relationship_line: usize,
    from: &str,
    to: &str,
) -> String {
    format!("{owner_id}:c4:{diagram_line}:rel:{relationship_line}:{from}:{to}")
}

fn symbol_kind(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Class => "class",
    }
}

fn language_name(language: Language) -> &'static str {
    match language {
        Language::Rust => "rust",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Python => "python",
        Language::Go => "go",
        Language::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::vault::Vault;

    #[test]
    fn disabled_source_index_writes_empty_source_state() {
        let root = unique_temp_dir("criv-disabled-source-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[index]
source = false
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();
        let json = serde_json::to_value(&state).unwrap();

        assert_eq!(json["source-index"].as_array().unwrap().len(), 0);
        assert!(
            json["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["kind"] != "code")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn state_and_snapshot_writes_are_parseable() {
        let root = unique_temp_dir("criv-state-atomic-writes");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        reset_work_counts();
        let (snapshot, _) = write_state(&root, &vault).unwrap();

        let state_path = root.join(".criv/state.json");
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
        assert_eq!(state["schema"], STATE_SCHEMA);

        let latest = std::fs::read_to_string(root.join(".criv/latest")).unwrap();
        assert_eq!(latest.trim(), snapshot);

        let snapshot_path = root
            .join(".criv/snapshots")
            .join(format!("{snapshot}.json"));
        let state_contents = std::fs::read_to_string(root.join(".criv/state.json")).unwrap();
        let snapshot_contents = std::fs::read_to_string(snapshot_path).unwrap();
        assert_eq!(state_contents, snapshot_contents);
        assert!(state_contents.ends_with('\n'));
        let snapshot_state: serde_json::Value = serde_json::from_str(&snapshot_contents).unwrap();
        assert_eq!(snapshot_state["schema"], STATE_SCHEMA);
        assert_eq!(
            work_counts(),
            WorkCounts {
                partitions_rebuilt: 2,
                source_partitions_rebuilt: 1,
                note_partitions_rebuilt: 0,
                c4_partitions_rebuilt: 0,
                policy_partitions_rebuilt: 0,
                source_index_partitions_rebuilt: 1,
                serializations: 1,
                published_bytes: state_contents.len() * 2 + latest.len(),
            }
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn serialized_state_matches_the_v0_contract_fixture() {
        let root = unique_temp_dir("criv-state-contract-fixture");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/adr/0001-entrypoint.md"),
            r#"---
id: ADR-0001
kind: decision
title: Entrypoint
status: accepted
governs:
  - src/lib.rs
policy:
  patterns:
    - id: entrypoint
      language: rust
      pattern: "fn $NAME() { $$$BODY }"
---

# Entrypoint
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("docs/adr/0002-draft-entrypoint.md"),
            r#"---
id: ADR-0002
kind: decision
title: Draft Entrypoint
status: draft
governs:
  - src/lib.rs
policy:
  patterns:
    - id: draft-entrypoint
      language: rust
      pattern: "fn $NAME() { $$$BODY }"
---

# Draft Entrypoint
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        reset_work_counts();
        let actual: serde_json::Value =
            serde_json::from_str(&State::build(&root, &vault).unwrap().to_json().unwrap()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/state/criv.state.v0.json")).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(
            actual["registered-patterns"],
            serde_json::json!(["ADR-0001/entrypoint"])
        );
        assert!(
            actual["patterns"]
                .get("ADR-0002/draft-entrypoint")
                .is_none()
        );
        assert_eq!(
            work_counts(),
            WorkCounts {
                partitions_rebuilt: 5,
                source_partitions_rebuilt: 1,
                note_partitions_rebuilt: 2,
                c4_partitions_rebuilt: 0,
                policy_partitions_rebuilt: 1,
                source_index_partitions_rebuilt: 1,
                serializations: 1,
                published_bytes: 0,
            }
        );
        assert_eq!(
            State::build(&root, &vault).unwrap().hash().unwrap(),
            "f8c73cf18a6e2419171693357e3fed14281733b6071ea92e8c490d7e90ed64ba"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn state_writes_reject_a_symlinked_criv_directory() {
        use std::os::unix::fs::symlink;

        let root = unique_temp_dir("criv-state-symlink");
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        symlink(outside.path(), root.join(".criv")).unwrap();

        let error = Vault::load(&root).unwrap_err();

        assert!(error.to_string().contains("symlinked vault path component"));
        assert!(!outside.path().join("state.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn c4_diagrams_are_written_to_graph_state() {
        let root = unique_temp_dir("criv-c4-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/c4.md"),
            r#"---
id: c4
kind: doc
title: C4
---

# C4

```mermaid
C4Container
System_Boundary(system, "criv") {
    Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
    %% criv:source src/main.rs
    Container(plugin, "Obsidian Plugin", "TypeScript", "Reads generated state")
}
System_Ext(github, "GitHub", "Hosts remote repositories")
Rel(cli, plugin, "writes state for")
```
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();

        let cli_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-container" && node.label == "criv CLI")
            .expect("c4 container node");
        let plugin_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-container" && node.label == "Obsidian Plugin")
            .expect("second c4 container node");
        let github_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-software-system" && node.label == "GitHub")
            .expect("external software system node");
        assert!(
            github_node
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with("docs/c4.md#L"))
        );
        assert!(
            !state.graph.nodes.iter().any(|node| {
                node.kind.starts_with("c4-")
                    && node.kind != "c4-relationship"
                    && node.label == "criv"
            }),
            "boundary labels must not be emitted as architecture element nodes"
        );
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == "note:c4" && edge.to == cli_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id && edge.to == "code:src/main.rs" && edge.kind == "references"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id && edge.to == plugin_node.id && edge.kind == "relates"
        }));
        let relationship_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-relationship" && node.label == "writes state for")
            .expect("labelled c4 relationship node");
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == "note:c4" && edge.to == relationship_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == relationship_node.id && edge.to == cli_node.id && edge.kind == "from"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == relationship_node.id && edge.to == plugin_node.id && edge.kind == "to"
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn c4_artifacts_are_written_to_graph_state() {
        let root = unique_temp_dir("criv-c4-artifact-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/architecture")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/architecture/02-container.c4"),
            r#"
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs#fn:run
Container(plugin, "Obsidian Plugin", "TypeScript", "Reads generated state")
Rel(cli, plugin, "writes state for")
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();

        let artifact_node = state
            .graph
            .nodes
            .iter()
            .find(|node| {
                node.kind == "c4-artifact"
                    && node.path.as_deref() == Some("docs/architecture/02-container.c4")
            })
            .expect("c4 artifact node");
        let diagram_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-diagram" && node.label == "container diagram")
            .expect("c4 diagram node");
        let cli_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-container" && node.label == "criv CLI")
            .expect("c4 container node");
        let relationship_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-relationship" && node.label == "writes state for")
            .expect("c4 relationship node");

        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == artifact_node.id && edge.to == diagram_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == diagram_node.id && edge.to == cli_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == diagram_node.id
                && edge.to == relationship_node.id
                && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id && edge.to == "code:src/main.rs" && edge.kind == "references"
        }));
        let interface_node = state
            .graph
            .nodes
            .iter()
            .find(|node| {
                node.kind == "c4-interface"
                    && node.path.as_deref() == Some("src/main.rs#fn:run")
                    && !node.label.is_empty()
            })
            .expect("c4 interface hash node");
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id
                && edge.to == interface_node.id
                && edge.kind == "tracks-interface"
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    const POLICY_ADR: &str = r#"---
id: ADR-0001
kind: decision
title: No Println
status: accepted
date: 2026-07-25
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Prefer structured diagnostics.
---

# No Println

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    const PATTERN_ID: &str = "ADR-0001/no-println";
    const DRAFT_PATTERN_ID: &str = "ADR-0002/no-debug";
    const FUNCTION_PATTERN_ID: &str = "ADR-0002/function";
    const FUNCTION_POLICY_ADR: &str = r#"---
id: ADR-0002
kind: decision
title: Functions Require Review
status: accepted
date: 2026-07-25
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: function
      language: rust
      pattern: "fn $NAME() { $$$BODY }"
---

# Functions Require Review

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    const DRAFT_POLICY_ADR: &str = r#"---
id: ADR-0002
kind: decision
title: No Debug
status: draft
date: 2026-08-02
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-debug
      language: rust
      pattern: "println!($$$ARGS)"
---

# No Debug

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    fn policy_vault(prefix: &str) -> PathBuf {
        let root = unique_temp_dir(prefix);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            "[vault]\ndocs = \"docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"src\"]\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/adr/0001-no-println.md"), POLICY_ADR).unwrap();
        std::fs::write(
            root.join("src/alpha.rs"),
            "fn alpha() {\n    println!(\"alpha\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/beta.rs"),
            "fn beta() {\n    println!(\"beta\");\n}\n",
        )
        .unwrap();
        root
    }

    fn matched_files(state: &State) -> Vec<String> {
        state
            .patterns
            .get(PATTERN_ID)
            .map(|matches| matches.iter().map(|matched| matched.file.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn no_op_incremental_build_reuses_partition_allocations() {
        let root = policy_vault("criv-state-partition-allocation-reuse");
        let vault = Vault::load(&root).unwrap();
        let first = State::build(&root, &vault).unwrap();

        reset_work_counts();
        let second = State::build_incremental(&root, &vault, Some(&first), &[]).unwrap();

        assert_eq!(work_counts().partitions_rebuilt, 0);
        for (path, partition) in &first.partitions.sources {
            assert!(Arc::ptr_eq(
                partition,
                second.partitions.sources.get(path).unwrap()
            ));
        }
        for (path, partition) in &first.partitions.notes {
            assert!(Arc::ptr_eq(
                partition,
                second.partitions.notes.get(path).unwrap()
            ));
        }
        for (id, partition) in &first.partitions.policies {
            assert!(Arc::ptr_eq(
                partition,
                second.partitions.policies.get(id).unwrap()
            ));
        }
        for (path, partition) in &first.partitions.source_index {
            assert!(Arc::ptr_eq(
                partition,
                second.partitions.source_index.get(path).unwrap()
            ));
        }
        assert_eq!(first.to_json().unwrap(), second.to_json().unwrap());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn state_batches_overlapping_adr_policies() {
        let root = policy_vault("criv-state-batched-policies");
        std::fs::write(
            root.join("docs/adr/0002-functions-require-review.md"),
            FUNCTION_POLICY_ADR,
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();
        structural::reset_batch_parse_count();
        let state = State::build(&root, &vault).unwrap();

        assert_eq!(
            structural::batch_parse_count(),
            2,
            "each eligible source file is parsed once for both overlapping ADR policies"
        );
        assert_eq!(matched_files(&state).len(), 2);
        assert_eq!(state.patterns.get(FUNCTION_PATTERN_ID).unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn state_publishes_accepted_policy_patterns_only() {
        let root = policy_vault("criv-accepted-only-state");
        std::fs::write(root.join("docs/adr/0002-no-debug.md"), DRAFT_POLICY_ADR).unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();

        assert_eq!(state.registered_patterns, vec![PATTERN_ID.to_string()]);
        assert!(state.patterns.contains_key(PATTERN_ID));
        assert!(
            !state
                .registered_patterns
                .contains(&DRAFT_PATTERN_ID.to_string())
        );
        assert!(!state.patterns.contains_key(DRAFT_PATTERN_ID));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_policy_promotion_scans_every_governed_source() {
        let root = policy_vault("criv-policy-promotion");
        let policy_path = root.join("docs/adr/0002-no-debug.md");
        std::fs::write(&policy_path, DRAFT_POLICY_ADR).unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, before) = write_state(&root, &vault).unwrap();
        assert!(!before.patterns.contains_key(DRAFT_PATTERN_ID));

        std::fs::write(
            &policy_path,
            DRAFT_POLICY_ADR.replace("status: draft", "status: accepted"),
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();
        let (_, after) = write_state_incremental(&root, &vault, Some(&before), &[]).unwrap();

        assert_eq!(
            after.patterns[DRAFT_PATTERN_ID]
                .iter()
                .map(|matched| matched.file.as_str())
                .collect::<Vec<_>>(),
            vec!["src/alpha.rs", "src/beta.rs"],
            "a newly accepted policy has no prior cache entry and must scan its whole scope"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_policy_demotion_removes_registration_and_cached_matches() {
        let root = policy_vault("criv-policy-demotion");
        let policy_path = root.join("docs/adr/0002-no-debug.md");
        let accepted = DRAFT_POLICY_ADR.replace("status: draft", "status: accepted");
        std::fs::write(&policy_path, &accepted).unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, before) = write_state(&root, &vault).unwrap();
        assert!(before.patterns.contains_key(DRAFT_PATTERN_ID));

        std::fs::write(&policy_path, DRAFT_POLICY_ADR).unwrap();
        let vault = Vault::load(&root).unwrap();
        let (_, after) = write_state_incremental(
            &root,
            &vault,
            Some(&before),
            &["docs/adr/0002-no-debug.md".to_string()],
        )
        .unwrap();

        assert!(
            !after
                .registered_patterns
                .contains(&DRAFT_PATTERN_ID.to_string())
        );
        assert!(!after.patterns.contains_key(DRAFT_PATTERN_ID));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_pattern_matches_reuse_unchanged_files() {
        let root = policy_vault("criv-incremental-pattern-reuse");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(
            matched_files(&first),
            vec!["src/alpha.rs".to_string(), "src/beta.rs".to_string()],
            "both governed files should match before the edit"
        );
        let alpha_before = first
            .patterns
            .get(PATTERN_ID)
            .unwrap()
            .iter()
            .find(|matched| matched.file == "src/alpha.rs")
            .cloned()
            .expect("alpha match");

        std::fs::write(
            root.join("src/beta.rs"),
            "fn beta() {\n    // moved down\n    println!(\"beta changed\");\n}\n",
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        assert_eq!(second.registered_patterns, vec![PATTERN_ID.to_string()]);

        let second_matches = second.patterns.get(PATTERN_ID).unwrap();
        let alpha_after = second_matches
            .iter()
            .find(|matched| matched.file == "src/alpha.rs")
            .expect("alpha match survives an unrelated edit");
        assert_eq!(
            &alpha_before, alpha_after,
            "an unchanged file's match must be carried forward byte-identically"
        );

        let beta_after = second_matches
            .iter()
            .find(|matched| matched.file == "src/beta.rs")
            .expect("beta match is rescanned");
        assert_ne!(
            beta_after.range, alpha_before.range,
            "the changed file's match should be rescanned at its new position"
        );
        assert_eq!(matched_files(&second).len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_pattern_matches_skip_files_absent_from_the_changed_set() {
        // Pins the reuse contract itself: a file the caller does not report as
        // changed is carried forward from the previous state and is NOT
        // rescanned. Editing alpha on disk while reporting only beta means a
        // full rescan would drop alpha's match, while correct reuse keeps it.
        let root = policy_vault("criv-incremental-pattern-scope");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::write(root.join("src/alpha.rs"), "fn alpha() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        assert!(
            matched_files(&second).contains(&"src/alpha.rs".to_string()),
            "a file outside the changed set must be reused, not rescanned"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_rescan_is_empty_when_no_changed_file_is_in_scope() {
        // `README.md` is outside the ADR's `governs:` scope, so the scoped
        // changed set is empty and an empty glob list must keep meaning "nothing
        // in scope". Widening it to "no filter" would rescan the whole vault and
        // pick up the println! added to alpha after the previous state was
        // written — a file the caller never reported as changed.
        let root = policy_vault("criv-incremental-pattern-out-of-scope");
        std::fs::write(root.join("src/alpha.rs"), "fn alpha() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(matched_files(&first), vec!["src/beta.rs".to_string()]);

        std::fs::write(
            root.join("src/alpha.rs"),
            "fn alpha() {\n    println!(\"alpha\");\n}\n",
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"README.md".to_string()),
        )
        .unwrap();

        assert_eq!(
            matched_files(&second),
            vec!["src/beta.rs".to_string()],
            "no governed file changed, so the rescan must contribute nothing"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_pattern_matches_drop_deleted_files() {
        let root = policy_vault("criv-incremental-pattern-delete");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::remove_file(root.join("src/beta.rs")).unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        assert_eq!(
            matched_files(&second),
            vec!["src/alpha.rs".to_string()],
            "a deleted file's match must not survive into the next state"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
    }
}

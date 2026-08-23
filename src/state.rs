use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(test)]
use std::{cell::Cell, thread_local};

#[cfg(test)]
use criv_state_wire::STATE_SCHEMA;
use criv_state_wire::{
    AssetIndexEntry, Edge, Graph, LikeC4ArchitectureState, Node, PatternMatch, SourceIndexEntry,
    StateDocument, source_identity::SourceIdentity,
};
use serde::{Serialize, Serializer};

use crate::c4::C4Artifact;
use crate::policy_scan::PolicyScanPlan;
use crate::source::{
    DirectiveKind, Import, Language, ModuleRelationshipRole, Relationship, RelationshipKind,
    RelationshipTarget, SourceFile, Symbol,
};
use crate::structural;
use crate::vault::{Note, NoteKind, ResolvedLink, SourceTargetResolution, Vault};
use crate::{CrivError, Result};

mod publication;
mod snapshots;

pub(crate) use publication::load_snapshot;

#[derive(Debug, Clone)]
pub(crate) struct State {
    wire: StateDocument,
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
pub(crate) struct ArchitectureInterfaceHashRecord {
    pub(crate) id: String,
    pub(crate) hash: String,
    pub(crate) path: String,
    pub(crate) source_path: String,
    pub(crate) line: usize,
    pub(crate) target: String,
}

impl State {
    #[cfg(test)]
    pub(crate) fn build(vault: &Vault) -> Result<Self> {
        let policy_plan = PolicyScanPlan::new(vault);
        Self::build_with_policy_plan(vault, None, &[], &policy_plan)
    }

    #[cfg(test)]
    fn build_incremental(
        vault: &Vault,
        previous: Option<&State>,
        changed_files: &[String],
    ) -> Result<Self> {
        let policy_plan = PolicyScanPlan::new(vault);
        Self::build_with_policy_plan(vault, previous, changed_files, &policy_plan)
    }

    fn build_with_policy_plan(
        vault: &Vault,
        previous: Option<&State>,
        changed_files: &[String],
        policy_plan: &PolicyScanPlan,
    ) -> Result<Self> {
        if let Some(error) = policy_plan.state_definition_error() {
            return Err(CrivError::new(error));
        }
        let partitions = StatePartitions::build(vault, previous, changed_files, policy_plan)?;
        let mut state = Self::from_partitions(partitions);
        state.wire.asset_index = vault
            .documentation_assets()
            .iter()
            .map(|asset| AssetIndexEntry {
                path: asset.path.clone(),
                mime: asset.mime.clone(),
                bytes: asset.bytes,
                hash: asset.hash.clone(),
            })
            .collect();
        state.wire.architecture = vault.likec4_workspace.model.clone().map(|model| {
            add_likec4_model_to_graph(&mut state.wire.graph, vault, &model);
            LikeC4ArchitectureState {
                protocol_version: 1,
                likec4_version: vault
                    .likec4_workspace
                    .version
                    .clone()
                    .unwrap_or_else(|| "1.59.2".into()),
                revision: 0,
                workspace: vault.likec4_workspace.path.clone(),
                model,
            }
        });
        Ok(state)
    }

    fn from_partitions(partitions: StatePartitions) -> Self {
        let (graph, patterns, source_index) = partitions.flatten();
        let registered_patterns = patterns.keys().cloned().collect();
        Self {
            wire: StateDocument::new(graph, registered_patterns, patterns, source_index),
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
        let _ = &self.partitions;
        #[cfg(test)]
        record_work(|counts| counts.serializations += 1);

        let json = serde_json::to_string_pretty(&self.wire)
            .map_err(|err| CrivError::new(format!("failed to serialize state: {err}")))?;
        Ok(SerializedState {
            hash: stable_hash(&json),
            published: format!("{json}\n"),
        })
    }

    fn publish_snapshot_with_check(
        &self,
        vault: &Vault,
        serialized: &SerializedState,
        keep: usize,
        precommit_check: impl FnOnce() -> Result<()>,
    ) -> Result<String> {
        publication::publish_with_precommit_check(
            vault.repository_files(),
            &serialized.hash,
            &serialized.published,
            keep,
            precommit_check,
        )?;
        #[cfg(test)]
        record_work(|counts| {
            counts.published_bytes += serialized.published.len() * 2 + serialized.hash.len() + 1;
        });
        Ok(serialized.hash.clone())
    }
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.wire.serialize(serializer)
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
        vault: &Vault,
        previous: Option<&State>,
        changed_files: &[String],
        policy_plan: &PolicyScanPlan,
    ) -> Result<Self> {
        let previous = previous.map(|state| &state.partitions);
        let policy_fingerprints = policy_fingerprints(policy_plan);
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
                    Arc::new(build_c4_artifact_partition(artifact))
                });
            partitions
                .c4_artifacts
                .insert(artifact.rel_path.clone(), partition);
        }

        for entry in vault.source_entries() {
            let state_entry = SourceIndexEntry {
                mime: source_mime(&entry.path),
                path: entry.path.clone(),
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
            partitions
                .source_index
                .insert(entry.path.clone(), partition);
        }

        partitions.policies = build_policy_partitions(vault, previous, changed_files, policy_plan)?;
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
    fingerprint_str(&mut hasher, file.language.as_str());
    for import in &file.imports {
        fingerprint_str(&mut hasher, &import.module);
        fingerprint_usize(&mut hasher, import.line);
        fingerprint_usize(&mut hasher, import.site);
        fingerprint_str(&mut hasher, import.kind.as_str());
        fingerprint_str(&mut hasher, &format!("{:?}", import.owner));
        fingerprint_str(&mut hasher, &format!("{:?}", import.scope));
        fingerprint_option_str(&mut hasher, import.alias.as_deref());
        fingerprint_str(&mut hasher, &format!("{:?}", import.only));
        fingerprint_str(&mut hasher, &format!("{:?}", import.except));
        fingerprint_bool(&mut hasher, import.absolute);
    }
    for symbol in &file.symbols {
        fingerprint_str(&mut hasher, &symbol.id.display());
        fingerprint_str(&mut hasher, &symbol.name);
        fingerprint_str(&mut hasher, symbol.kind.as_str());
        fingerprint_option_str(&mut hasher, symbol.parent.as_deref());
        fingerprint_str(&mut hasher, &format!("{:?}", symbol.owner));
        fingerprint_option_usize(&mut hasher, symbol.arity);
        fingerprint_usize(&mut hasher, symbol.range.start_line);
        fingerprint_usize(&mut hasher, symbol.range.end_line);
        for call in &symbol.calls {
            fingerprint_str(&mut hasher, &call.target);
            fingerprint_usize(&mut hasher, call.line);
        }
        for relationship in &symbol.relationships {
            fingerprint_str(&mut hasher, relationship.kind.as_str());
            fingerprint_str(&mut hasher, &format!("{:?}", relationship.target));
            fingerprint_usize(&mut hasher, relationship.line);
            fingerprint_usize(&mut hasher, relationship.site);
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
    stable_hash(&format!("{}\0{:?}", entry.path, entry.mime))
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

fn fingerprint_option_usize(hasher: &mut blake3::Hasher, value: Option<usize>) {
    match value {
        Some(value) => {
            hasher.update(&[1]);
            fingerprint_usize(hasher, value);
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

fn fingerprint_bool(hasher: &mut blake3::Hasher, value: bool) {
    hasher.update(&[u8::from(value)]);
}

fn observe_partition_meta(meta: &PartitionMeta, expected_key: &PartitionKey) {
    debug_assert_eq!(&meta.key, expected_key);
    let _ = (
        &meta.input_fingerprint,
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
    let file_id = code_node_id(&file.path);

    for import in &file.imports {
        let import_id = directive_node_id(file, import);
        add_node(
            &mut graph,
            &mut seen_nodes,
            Node {
                id: import_id.clone(),
                hash: String::new(),
                kind: import.kind.as_str().into(),
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
                kind: symbol.kind.as_str().into(),
                label: symbol_label(symbol),
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
            add_edge(
                &mut graph,
                &mut seen_edges,
                &symbol_node_id(&parent_id.display()),
                &symbol_id,
                "contains",
            );
        }
        if let Some(owner) = &symbol.owner
            && let Some(parent) = file.symbols.iter().find(|candidate| {
                candidate.id != symbol.id
                    && candidate.arity.is_none()
                    && candidate.owner.as_ref() == Some(owner)
            })
        {
            add_edge(
                &mut graph,
                &mut seen_edges,
                &symbol_node_id(&parent.id.display()),
                &symbol_id,
                "contains",
            );
        }
        for call in &symbol.calls {
            dependencies.call_targets.insert(call.target.clone());
            let resolved = vault.source_graph().resolve_call(&symbol.id, &call.target);
            if resolved.is_none() {
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
        for relationship in &symbol.relationships {
            project_relationship(
                vault,
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                &mut dependencies,
                symbol,
                relationship,
            );
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
            label: format!("{} ({})", file.path, file.language.as_str()),
            path: Some(file.path.clone()),
        },
        rows: graph_rows(graph),
    }
}

fn project_relationship(
    vault: &Vault,
    graph: &mut Graph,
    seen_nodes: &mut BTreeSet<String>,
    seen_edges: &mut BTreeSet<String>,
    dependencies: &mut PartitionDependencies,
    symbol: &Symbol,
    relationship: &Relationship,
) {
    let source_graph = vault.source_graph();
    let label = match &relationship.target {
        RelationshipTarget::Dynamic { label, .. } => label.clone(),
        _ => source_graph.relationship_target_label(&symbol.id, relationship),
    };
    let resolved = source_graph.resolve_relationship(&symbol.id, relationship);
    if let Some(target) = resolved
        .as_ref()
        .and_then(|target| source_graph.symbol_name(target))
    {
        dependencies.call_targets.insert(target.to_string());
    } else if let RelationshipTarget::Callable { name, .. } = &relationship.target {
        dependencies.call_targets.insert(name.clone());
    } else if let RelationshipTarget::Module { module, .. } = &relationship.target {
        dependencies.call_targets.insert(module.clone());
    }
    if resolved.is_none() && !matches!(relationship.target, RelationshipTarget::Dynamic { .. }) {
        dependencies.catalog_sensitive = true;
    }
    let target = resolved
        .as_ref()
        .map(|target| symbol_node_id(&target.display()))
        .unwrap_or_else(|| relationship_target_node_id(relationship, &label));
    if resolved.is_none() {
        add_node(
            graph,
            seen_nodes,
            Node {
                id: target.clone(),
                hash: String::new(),
                kind: relationship_target_node_kind(relationship).into(),
                label,
                path: Some(format!("{}#L{}", symbol.id.path, relationship.line)),
            },
        );
    }
    add_edge(
        graph,
        seen_edges,
        &symbol_node_id(&symbol.id.display()),
        &target,
        relationship_edge_kind(relationship),
    );
}

fn relationship_target_node_id(relationship: &Relationship, label: &str) -> String {
    match &relationship.target {
        RelationshipTarget::Dynamic { id, .. } => format!("dynamic-call:{id}"),
        RelationshipTarget::Callable { .. } => external_call_node_id(label),
        RelationshipTarget::Module { .. } => external_module_node_id(label),
    }
}

fn relationship_target_node_kind(relationship: &Relationship) -> &'static str {
    match &relationship.target {
        RelationshipTarget::Dynamic { .. } => "dynamic-call",
        RelationshipTarget::Callable { .. } => "external-call",
        RelationshipTarget::Module { .. } => "external-module",
    }
}

fn relationship_edge_kind(relationship: &Relationship) -> &'static str {
    match (&relationship.kind, &relationship.target) {
        (RelationshipKind::Call, _) => "calls",
        (RelationshipKind::Capture, _) => "captures",
        (RelationshipKind::Delegate, _) => "delegates",
        (
            RelationshipKind::ProtocolImplementation,
            RelationshipTarget::Module {
                role: ModuleRelationshipRole::Protocol,
                ..
            },
        ) => "implements-protocol",
        (
            RelationshipKind::ProtocolImplementation,
            RelationshipTarget::Module {
                role: ModuleRelationshipRole::ForType,
                ..
            },
        ) => "implements-for",
        (RelationshipKind::BehaviourImplementation, _) => "implements-behaviour",
        _ => relationship.kind.as_str(),
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
                dependencies.source_content_paths.insert(path);
            }
            SourceTargetResolution::MissingFile => {}
        }
    }

    for heading in &note.headings {
        let heading_id = format!("{note_id}#{}", crate::identity::kebab(&heading.text));
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
                    dependencies.source_content_paths.insert(path);
                }
            }
        }
    }

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

fn build_c4_artifact_partition(artifact: &C4Artifact) -> RowPartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let artifact_id = c4_artifact_node_id(&artifact.rel_path);
    add_node(
        &mut graph,
        &mut seen_nodes,
        Node {
            id: artifact_id.clone(),
            hash: String::new(),
            kind: "architecture-source".into(),
            label: artifact.rel_path.clone(),
            path: Some(artifact.rel_path.clone()),
        },
    );
    RowPartition {
        meta: partition_meta(
            PartitionKey::C4Artifact(artifact.rel_path.clone()),
            c4_artifact_input_fingerprint(artifact),
            PartitionDependencies::default(),
        ),
        rows: graph_rows(graph),
    }
}

fn collect_graph_source_dependencies(graph: &Graph, dependencies: &mut PartitionDependencies) {
    for node in &graph.nodes {
        if node.kind == "architecture-interface"
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
    vault: &Vault,
    previous: Option<&StatePartitions>,
    changed_files: &[String],
    policy_plan: &PolicyScanPlan,
) -> Result<BTreeMap<String, Arc<PolicyPartition>>> {
    let mut partitions = BTreeMap::new();
    let mut pending_policy_scans = Vec::new();
    for owner in policy_plan.owners() {
        let changed_paths = changed_paths_in_scopes(changed_files, owner.scopes())
            .into_iter()
            .collect::<BTreeSet<_>>();
        for policy in owner.policies() {
            let Some(pattern_id) = policy.state_pattern_id() else {
                continue;
            };
            let input_fingerprint = policy
                .state_input_fingerprint()
                .expect("published policies have an input fingerprint")
                .to_string();
            let previous_partition =
                previous.and_then(|previous| previous.policies.get(pattern_id));
            let definition_unchanged = previous_partition
                .is_some_and(|partition| partition.meta.input_fingerprint == input_fingerprint);
            let paths = if definition_unchanged {
                changed_paths.clone()
            } else {
                owner.paths().clone()
            };

            if definition_unchanged && paths.is_empty() {
                partitions.insert(
                    pattern_id.to_string(),
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
                pattern_id: pattern_id.to_string(),
                input_fingerprint,
                policy: policy.compiled(),
                paths,
                reused,
            });
        }
    }

    let requests = pending_policy_scans
        .iter()
        .enumerate()
        .map(|(key, scan)| structural::PolicyScanRequest {
            key,
            policy: scan.policy,
            paths: &scan.paths,
        })
        .collect::<Vec<_>>();
    let rescanned = structural::find_policies_batch(vault, &requests)?;
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

fn policy_fingerprints(policy_plan: &PolicyScanPlan) -> BTreeMap<String, String> {
    policy_plan
        .owners()
        .iter()
        .flat_map(|owner| owner.policies())
        .filter_map(|policy| {
            Some((
                policy.state_pattern_id()?.to_string(),
                policy.state_input_fingerprint()?.to_string(),
            ))
        })
        .collect()
}

#[cfg(test)]
fn write_state(vault: &Vault) -> Result<(String, State)> {
    let policy_plan = PolicyScanPlan::new(vault);
    write_state_with_policy_plan_and_check(vault, &policy_plan, || Ok(()))
}

pub(crate) fn write_state_with_policy_plan_and_check(
    vault: &Vault,
    policy_plan: &PolicyScanPlan,
    precommit_check: impl FnOnce() -> Result<()>,
) -> Result<(String, State)> {
    let state = State::build_with_policy_plan(vault, None, &[], policy_plan)?;
    let serialized = state.serialize()?;
    let snapshot = state.publish_snapshot_with_check(
        vault,
        &serialized,
        vault.config.state_keep,
        precommit_check,
    )?;
    Ok((snapshot, state))
}

#[cfg(test)]
fn write_state_incremental(
    vault: &Vault,
    previous: Option<&State>,
    changed_files: &[String],
) -> Result<(String, State)> {
    let policy_plan = PolicyScanPlan::new(vault);
    write_state_incremental_with_policy_plan_and_check(
        vault,
        previous,
        changed_files,
        &policy_plan,
        || Ok(()),
    )
}

pub(crate) fn write_state_incremental_with_policy_plan_and_check(
    vault: &Vault,
    previous: Option<&State>,
    changed_files: &[String],
    policy_plan: &PolicyScanPlan,
    precommit_check: impl FnOnce() -> Result<()>,
) -> Result<(String, State)> {
    let state = State::build_with_policy_plan(vault, previous, changed_files, policy_plan)?;
    let serialized = state.serialize()?;
    let snapshot = state.publish_snapshot_with_check(
        vault,
        &serialized,
        vault.config.state_keep,
        precommit_check,
    )?;
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
    policy: &'a structural::CompiledPolicy,
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

pub(crate) fn architecture_interface_hash_records(
    vault: &Vault,
) -> Vec<ArchitectureInterfaceHashRecord> {
    let mut records = Vec::new();
    collect_likec4_interface_hashes(vault, &mut records);
    records
}

impl State {
    pub(crate) fn architecture_interface_hashes(&self) -> BTreeMap<String, String> {
        self.wire
            .graph
            .nodes
            .iter()
            .filter(|node| node.kind == "architecture-interface")
            .map(|node| (node.id.clone(), node.label.clone()))
            .collect()
    }
}

fn add_likec4_model_to_graph(graph: &mut Graph, vault: &Vault, model: &serde_json::Value) {
    let mut seen_nodes = graph
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let mut seen_edges = graph
        .edges
        .iter()
        .map(|edge| format!("{}\0{}\0{}", edge.from, edge.to, edge.kind))
        .collect::<BTreeSet<_>>();
    let workspace_id = "architecture:likec4";
    add_node(
        graph,
        &mut seen_nodes,
        Node {
            id: workspace_id.into(),
            hash: String::new(),
            kind: "architecture-workspace".into(),
            label: "LikeC4 architecture".into(),
            path: Some(vault.likec4_workspace.path.clone()),
        },
    );

    for element in model_array(model, "elements") {
        let Some(id) = element.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let element_id = likec4_element_node_id(id);
        add_node(
            graph,
            &mut seen_nodes,
            Node {
                id: element_id.clone(),
                hash: String::new(),
                kind: "architecture-element".into(),
                label: element
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(id)
                    .to_string(),
                path: Some(vault.likec4_workspace.path.clone()),
            },
        );
        add_edge(
            graph,
            &mut seen_edges,
            workspace_id,
            &element_id,
            "contains",
        );
    }

    for relationship in model_array(model, "relationships") {
        let Some(source) = relationship_endpoint(relationship, "source") else {
            continue;
        };
        let Some(target) = relationship_endpoint(relationship, "target") else {
            continue;
        };
        add_edge(
            graph,
            &mut seen_edges,
            &likec4_element_node_id(source),
            &likec4_element_node_id(target),
            "relates",
        );
    }

    for link in model_array(model, "sourceLinks") {
        let Some(element) = link.get("element").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = link.get("target").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(target)
        else {
            continue;
        };
        let element_id = likec4_element_node_id(element);
        add_edge(
            graph,
            &mut seen_edges,
            &element_id,
            &code_node_id(&path),
            "references",
        );
        if let Some((target, interface_hash)) = interface_anchor_hash(vault, target, &path) {
            let interface_id = likec4_interface_node_id(element);
            add_node(
                graph,
                &mut seen_nodes,
                Node {
                    id: interface_id.clone(),
                    hash: String::new(),
                    kind: "architecture-interface".into(),
                    label: interface_hash,
                    path: Some(target),
                },
            );
            add_edge(
                graph,
                &mut seen_edges,
                &element_id,
                &interface_id,
                "tracks-interface",
            );
        }
    }

    graph.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    graph.edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.kind).cmp(&(&right.from, &right.to, &right.kind))
    });
    graph.root = graph_root(graph);
}

fn collect_likec4_interface_hashes(
    vault: &Vault,
    records: &mut Vec<ArchitectureInterfaceHashRecord>,
) {
    let Some(model) = &vault.likec4_workspace.model else {
        return;
    };
    for link in model_array(model, "sourceLinks") {
        let Some(element) = link.get("element").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(source) = link.get("target").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(source)
        else {
            continue;
        };
        let Some((target, hash)) = interface_anchor_hash(vault, source, &path) else {
            continue;
        };
        records.push(ArchitectureInterfaceHashRecord {
            id: likec4_interface_node_id(element),
            hash,
            path: vault.likec4_workspace.path.clone(),
            source_path: path,
            line: 1,
            target,
        });
    }
}

fn model_array<'a>(model: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    model
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn relationship_endpoint<'a>(relationship: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    relationship.get(key)?.get("model")?.as_str()
}

fn likec4_element_node_id(element: &str) -> String {
    format!("architecture:element:{element}")
}

fn likec4_interface_node_id(element: &str) -> String {
    format!("architecture:interface:{element}")
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
    format!("code:{}", SourceIdentity::file(path))
}

fn pattern_node_id(id: &str) -> String {
    format!("pattern:{id}")
}

fn import_node_id(path: &str, module: &str) -> String {
    format!("import:{path}:{module}")
}

fn directive_node_id(file: &SourceFile, directive: &Import) -> String {
    if file.language != Language::Elixir || directive.kind == DirectiveKind::Legacy {
        return import_node_id(&file.path, &directive.module);
    }
    format!(
        "import:{}:{}:{}:{}",
        file.path,
        directive.kind.as_str(),
        directive.module,
        directive.site
    )
}

fn symbol_node_id(id: &str) -> String {
    format!("symbol:{id}")
}

fn external_call_node_id(id: &str) -> String {
    format!("external-call:{id}")
}

fn external_module_node_id(id: &str) -> String {
    format!("external-module:{id}")
}

fn c4_artifact_node_id(path: &str) -> String {
    format!("architecture-source:{path}")
}

fn symbol_label(symbol: &Symbol) -> String {
    symbol.arity.map_or_else(
        || symbol.name.clone(),
        |arity| format!("{}/{arity}", symbol.name),
    )
}

fn source_mime(path: &str) -> Option<String> {
    if let Some(mime) = Language::from_path(path).mime() {
        return Some(mime.into());
    }
    mime_guess::from_path(path).first_raw().map(str::to_string)
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
        let state = State::build(&vault).unwrap();
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
    fn state_publishes_verified_documentation_assets() {
        let root = unique_temp_dir("criv-documentation-asset-state");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let contents = b"\x89PNG\r\n\x1a\npreview";
        std::fs::write(root.join("docs/diagram.png"), contents).unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&vault).unwrap();
        let json = serde_json::to_value(&state).unwrap();

        assert_eq!(json["asset-index"][0]["path"], "docs/diagram.png");
        assert_eq!(json["asset-index"][0]["mime"], "image/png");
        assert_eq!(json["asset-index"][0]["bytes"], contents.len());
        assert_eq!(json["asset-index"][0]["hash"].as_str().unwrap().len(), 64);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn elixir_state_projects_kinds_relationships_labels_mime_and_fingerprints() {
        let root = unique_temp_dir("criv-elixir-state");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"lib\"]\n").unwrap();
        std::fs::write(root.join("lib/mix.exs"), "value = :ok\n").unwrap();
        let source = r#"
defmodule Demo.Behaviour do
  @callback run(term()) :: term()
  @macrocallback build(term()) :: term()
end

defprotocol Demo.Protocol do
  def work(value)
end

defmodule Demo.Target do
  def fetch(value), do: value
  def other(value), do: value
end

defmodule Demo.Record do
  defstruct [:value]
end

defmodule Demo.Error do
  defexception [:message]
end

defmodule Demo.App do
  alias Demo.Target, as: Target
  import Demo.Target, only: [fetch: 1]
  require Demo.Target
  use Demo.Target
  @behaviour Demo.Behaviour
  @behaviour External.Behaviour

  def run(value) do
    Target.fetch(value)
    External.fetch(value)
  end
  def capture(), do: &Target.fetch/1
  def dynamic(fun, value), do: fun.(value)
  defdelegate delegated(value), to: Target, as: :fetch
  defmacro build(value), do: value
  defguard valid(value) when is_integer(value)
end

defimpl Demo.Protocol, for: Demo.App do
  def work(value), do: value
end
"#;
        std::fs::write(root.join("lib/sample.ex"), source).unwrap();

        let vault = Vault::load(&root).unwrap();
        let file = vault.source_graph().files.get("lib/sample.ex").unwrap();
        let first_fingerprint = source_input_fingerprint(file);
        let first = State::build(&vault).unwrap();
        let decoded: StateDocument = serde_json::from_str(&first.to_json().unwrap()).unwrap();
        assert_eq!(decoded.schema, STATE_SCHEMA);

        for kind in [
            "module",
            "behaviour",
            "protocol",
            "implementation",
            "struct",
            "exception",
            "function",
            "macro",
            "guard",
            "callback",
            "macro-callback",
            "alias",
            "import",
            "require",
            "use",
            "dynamic-call",
            "external-call",
            "external-module",
        ] {
            assert!(
                first.wire.graph.nodes.iter().any(|node| node.kind == kind),
                "missing State node kind {kind}"
            );
        }
        for label in [
            "Demo.App",
            "Demo.Protocol for Demo.App",
            "run/1",
            "capture/0",
            "delegated/1",
        ] {
            assert!(
                first
                    .wire
                    .graph
                    .nodes
                    .iter()
                    .any(|node| node.label == label),
                "missing State label {label}"
            );
        }
        for kind in [
            "contains",
            "imports",
            "calls",
            "captures",
            "delegates",
            "implements-protocol",
            "implements-for",
            "implements-behaviour",
        ] {
            assert!(
                first.wire.graph.edges.iter().any(|edge| edge.kind == kind),
                "missing State edge kind {kind}"
            );
        }
        let app = "symbol:lib/sample.ex#module:Demo.App";
        let run = "symbol:lib/sample.ex#module:Demo.App/fn:run/1";
        assert!(
            first
                .wire
                .graph
                .edges
                .iter()
                .any(|edge| { edge.from == app && edge.to == run && edge.kind == "contains" })
        );
        assert!(
            first
                .wire
                .graph
                .nodes
                .iter()
                .all(|node| !node.hash.is_empty())
        );
        assert!(
            first
                .wire
                .graph
                .edges
                .iter()
                .all(|edge| !edge.hash.is_empty())
        );
        for path in ["lib/sample.ex", "lib/mix.exs"] {
            assert_eq!(
                first
                    .wire
                    .source_index
                    .iter()
                    .find(|entry| entry.path == path)
                    .and_then(|entry| entry.mime.as_deref()),
                Some("text/x-elixir")
            );
        }

        std::fs::write(
            root.join("lib/sample.ex"),
            source.replace("Target.fetch(value)", "Target.other(value)"),
        )
        .unwrap();
        let changed_vault = Vault::load(&root).unwrap();
        let changed_file = changed_vault
            .source_graph()
            .files
            .get("lib/sample.ex")
            .unwrap();
        assert_ne!(first_fingerprint, source_input_fingerprint(changed_file));

        reset_work_counts();
        let changed =
            State::build_incremental(&changed_vault, Some(&first), &["lib/sample.ex".into()])
                .unwrap();
        assert_eq!(work_counts().source_partitions_rebuilt, 1);
        assert!(changed.wire.graph.edges.iter().any(|edge| {
            edge.from == run
                && edge.to == "symbol:lib/sample.ex#module:Demo.Target/fn:other/1"
                && edge.kind == "calls"
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn likec4_model_adds_architecture_relationships_and_source_edges() {
        let root = unique_temp_dir("criv-likec4-graph");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"src\"]\n").unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn run(input: String) -> usize { input.len() }\n",
        )
        .unwrap();
        let mut vault = Vault::load(&root).unwrap();
        vault.likec4_workspace.path = "docs/architecture".into();
        vault.likec4_workspace.model = Some(serde_json::json!({
            "elements": [
                { "id": "app", "title": "Application" },
                { "id": "app.cli", "title": "CLI" }
            ],
            "relationships": [{
                "source": { "model": "app.cli" },
                "target": { "model": "app" }
            }],
            "sourceLinks": [{
                "element": "app.cli",
                "target": "src/lib.rs#fn:run"
            }]
        }));

        let state = State::build(&vault).unwrap();
        let json = serde_json::to_value(&state).unwrap();
        let nodes = json["graph"]["nodes"].as_array().unwrap();
        let edges = json["graph"]["edges"].as_array().unwrap();

        assert!(nodes.iter().any(|node| {
            node["id"] == "architecture:element:app.cli" && node["kind"] == "architecture-element"
        }));
        assert!(nodes.iter().any(|node| {
            node["id"] == "architecture:interface:app.cli"
                && node["kind"] == "architecture-interface"
        }));
        assert!(edges.iter().any(|edge| {
            edge["from"] == "architecture:element:app.cli"
                && edge["to"] == "architecture:element:app"
                && edge["kind"] == "relates"
        }));
        assert!(edges.iter().any(|edge| {
            edge["from"] == "architecture:element:app.cli"
                && edge["to"] == "code:src/lib.rs"
                && edge["kind"] == "references"
        }));

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
        let (snapshot, _) = write_state(&vault).unwrap();

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
    fn corrupt_snapshot_preflight_keeps_the_prior_state_authoritative() {
        let root = unique_temp_dir("criv-state-publication-preflight");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"src\"]\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
        let first_vault = Vault::load(&root).unwrap();
        write_state(&first_vault).unwrap();
        let prior_state = std::fs::read(root.join(".criv/state.json")).unwrap();

        let corrupt_hash = "a".repeat(64);
        std::fs::write(
            root.join(".criv/snapshots")
                .join(format!("{corrupt_hash}.json")),
            "{}\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn second() {}\n").unwrap();
        let second_vault = Vault::load(&root).unwrap();

        let error = write_state(&second_vault).unwrap_err();
        assert!(error.to_string().contains("corrupt"));
        assert_eq!(
            std::fs::read(root.join(".criv/state.json")).unwrap(),
            prior_state
        );
        assert!(
            root.join(".criv/snapshots")
                .join(format!("{corrupt_hash}.json"))
                .exists()
        );
    }

    #[test]
    fn serialized_state_matches_the_v1_contract_fixture() {
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
            serde_json::from_str(&State::build(&vault).unwrap().to_json().unwrap()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/state/criv.state.v1.json")).unwrap();

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
            State::build(&vault).unwrap().hash().unwrap(),
            "e726e1970e996838a7c68bf68cee6dc2bdd98ad6657e196f2bf44640dcb040c1"
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
            .wire
            .patterns
            .get(PATTERN_ID)
            .map(|matches| matches.iter().map(|matched| matched.file.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn no_op_incremental_build_reuses_partition_allocations() {
        let root = policy_vault("criv-state-partition-allocation-reuse");
        let vault = Vault::load(&root).unwrap();
        let first = State::build(&vault).unwrap();

        reset_work_counts();
        let second = State::build_incremental(&vault, Some(&first), &[]).unwrap();

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
        let state = State::build(&vault).unwrap();

        assert_eq!(
            structural::batch_parse_count(),
            2,
            "each eligible source file is parsed once for both overlapping ADR policies"
        );
        assert_eq!(matched_files(&state).len(), 2);
        assert_eq!(
            state.wire.patterns.get(FUNCTION_PATTERN_ID).unwrap().len(),
            2
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn state_publishes_accepted_policy_patterns_only() {
        let root = policy_vault("criv-accepted-only-state");
        std::fs::write(root.join("docs/adr/0002-no-debug.md"), DRAFT_POLICY_ADR).unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&vault).unwrap();

        assert_eq!(state.wire.registered_patterns, vec![PATTERN_ID.to_string()]);
        assert!(state.wire.patterns.contains_key(PATTERN_ID));
        assert!(
            !state
                .wire
                .registered_patterns
                .contains(&DRAFT_PATTERN_ID.to_string())
        );
        assert!(!state.wire.patterns.contains_key(DRAFT_PATTERN_ID));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_successor_removes_superseded_policy_state() {
        let root = policy_vault("criv-effective-policy-state");
        let vault = Vault::load(&root).unwrap();
        let (_, before) = write_state(&vault).unwrap();
        assert!(before.wire.patterns.contains_key(PATTERN_ID));

        let successor = root.join("docs/adr/0002-successor.md");
        std::fs::write(
            &successor,
            r#"---
id: ADR-0002
kind: decision
title: Retire the old policy
status: accepted
supersedes:
  - ADR-0001
governs:
  - src/**
---

# Retire the old policy
"#,
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();
        let (_, after) = write_state_incremental(
            &vault,
            Some(&before),
            &["docs/adr/0002-successor.md".to_string()],
        )
        .unwrap();

        assert!(
            !after
                .wire
                .registered_patterns
                .contains(&PATTERN_ID.to_string())
        );
        assert!(!after.wire.patterns.contains_key(PATTERN_ID));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_policy_promotion_scans_every_governed_source() {
        let root = policy_vault("criv-policy-promotion");
        let policy_path = root.join("docs/adr/0002-no-debug.md");
        std::fs::write(&policy_path, DRAFT_POLICY_ADR).unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, before) = write_state(&vault).unwrap();
        assert!(!before.wire.patterns.contains_key(DRAFT_PATTERN_ID));

        std::fs::write(
            &policy_path,
            DRAFT_POLICY_ADR.replace("status: draft", "status: accepted"),
        )
        .unwrap();
        let vault = Vault::load(&root).unwrap();
        let (_, after) = write_state_incremental(&vault, Some(&before), &[]).unwrap();

        assert_eq!(
            after.wire.patterns[DRAFT_PATTERN_ID]
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
        let (_, before) = write_state(&vault).unwrap();
        assert!(before.wire.patterns.contains_key(DRAFT_PATTERN_ID));

        std::fs::write(&policy_path, DRAFT_POLICY_ADR).unwrap();
        let vault = Vault::load(&root).unwrap();
        let (_, after) = write_state_incremental(
            &vault,
            Some(&before),
            &["docs/adr/0002-no-debug.md".to_string()],
        )
        .unwrap();

        assert!(
            !after
                .wire
                .registered_patterns
                .contains(&DRAFT_PATTERN_ID.to_string())
        );
        assert!(!after.wire.patterns.contains_key(DRAFT_PATTERN_ID));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_pattern_matches_reuse_unchanged_files() {
        let root = policy_vault("criv-incremental-pattern-reuse");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&vault).unwrap();
        assert_eq!(
            matched_files(&first),
            vec!["src/alpha.rs".to_string(), "src/beta.rs".to_string()],
            "both governed files should match before the edit"
        );
        let alpha_before = first
            .wire
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
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        assert_eq!(
            second.wire.registered_patterns,
            vec![PATTERN_ID.to_string()]
        );

        let second_matches = second.wire.patterns.get(PATTERN_ID).unwrap();
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
        let (_, first) = write_state(&vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::write(root.join("src/alpha.rs"), "fn alpha() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
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
        let (_, first) = write_state(&vault).unwrap();
        assert_eq!(matched_files(&first), vec!["src/beta.rs".to_string()]);

        std::fs::write(
            root.join("src/alpha.rs"),
            "fn alpha() {\n    println!(\"alpha\");\n}\n",
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
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
        let (_, first) = write_state(&vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::remove_file(root.join("src/beta.rs")).unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
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

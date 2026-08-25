use super::{
    Arc, BTreeMap, BTreeSet, Graph, PartitionDependencies, PartitionKey, PartitionKind,
    PartitionMeta, PatternMatch, PendingPolicyScan, PolicyPartition, PolicyScanPlan, Result,
    SourceIndexEntry, SourceIndexPartition, State, StatePartitions, Vault, add_node,
    append_graph_rows, c4_artifact_input_fingerprint, changed_paths_in_scopes, graph_root,
    note_catalog_fingerprint, note_input_fingerprint, observe_partition_meta, partition_meta,
    pattern_match_from_structural, record_partition_rebuilt, reusable_matches,
    sort_and_dedup_pattern_matches, source_index_input_fingerprint, source_mime, structural,
};

#[derive(Debug, Clone, Default)]
pub(super) struct ReverseDependencies {
    source_content: BTreeMap<String, BTreeSet<PartitionKey>>,
    call_target: BTreeMap<String, BTreeSet<PartitionKey>>,
    source_catalog: BTreeSet<PartitionKey>,
    note_catalog: BTreeSet<PartitionKey>,
    policy_catalog: BTreeSet<PartitionKey>,
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

pub(super) fn build(
    vault: &Vault,
    previous: Option<&State>,
    changed_files: &[String],
    policy_plan: &PolicyScanPlan,
) -> Result<StatePartitions> {
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
    let mut partitions = StatePartitions::default();

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
                Arc::new(super::projection::build_source_partition(vault, file))
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
                Arc::new(super::projection::build_note_partition(vault, note))
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
                Arc::new(super::projection::build_c4_artifact_partition(artifact))
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

pub(super) fn flatten(
    partitions: &StatePartitions,
) -> (
    Graph,
    BTreeMap<String, Vec<PatternMatch>>,
    Vec<SourceIndexEntry>,
) {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();

    // Keep the public v0 ordering: every code file node precedes source details.
    for (path, partition) in &partitions.sources {
        observe_partition_meta(&partition.meta, &PartitionKey::Source(path.clone()));
        add_node(&mut graph, &mut seen_nodes, partition.code_node.clone());
    }
    for partition in partitions.sources.values() {
        append_graph_rows(
            &mut graph,
            &mut seen_nodes,
            &mut seen_edges,
            &partition.rows,
        );
    }
    for (path, partition) in &partitions.notes {
        observe_partition_meta(&partition.meta, &PartitionKey::Note(path.clone()));
        append_graph_rows(
            &mut graph,
            &mut seen_nodes,
            &mut seen_edges,
            &partition.rows,
        );
    }
    for (path, partition) in &partitions.c4_artifacts {
        observe_partition_meta(&partition.meta, &PartitionKey::C4Artifact(path.clone()));
        append_graph_rows(
            &mut graph,
            &mut seen_nodes,
            &mut seen_edges,
            &partition.rows,
        );
    }
    graph.root = graph_root(&graph);

    let patterns = partitions
        .policies
        .iter()
        .map(|(id, partition)| {
            observe_partition_meta(&partition.meta, &PartitionKey::Policy(id.clone()));
            (id.clone(), partition.matches.clone())
        })
        .collect();
    let source_index = partitions
        .source_index
        .iter()
        .map(|(path, partition)| {
            observe_partition_meta(&partition.meta, &PartitionKey::SourceIndex(path.clone()));
            partition.entry.clone()
        })
        .collect();

    (graph, patterns, source_index)
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
        .flat_map(super::super::policy_scan::PlannedOwner::policies)
        .filter_map(|policy| {
            Some((
                policy.state_pattern_id()?.to_string(),
                policy.state_input_fingerprint()?.to_string(),
            ))
        })
        .collect()
}

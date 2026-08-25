use super::*;

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

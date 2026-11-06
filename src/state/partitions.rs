//! State partitions: the rows of one Source file, one note, or one registered
//! policy, each with the fingerprint and dependencies that decide reuse.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use criv_state_wire::{Graph, PatternMatch};

use super::projection::{self, GraphBuilder, NoteRows, PartitionDependencies, SourceRows};
use super::stable_hash::StableHasher;
use crate::glob::GlobMatcher;
use crate::policy_scan::PolicyScanPlan;
use crate::structural::{self, CompiledPolicy, PolicyPattern, StructuralMatch};
use crate::vault::{DecisionStatus, Note, Vault};
#[cfg(test)]
use crate::work::{self, Work};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Default)]
pub(super) struct StatePartitions {
    sources: BTreeMap<String, Arc<SourcePartition>>,
    notes: BTreeMap<String, Arc<NotePartition>>,
    policies: BTreeMap<String, Arc<PolicyPartition>>,
    reverse_dependencies: ReverseDependencies,
    note_catalog_fingerprint: String,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
enum PartitionKey {
    Source(String),
    Note(String),
}

#[derive(Debug, Clone)]
pub(super) struct SourcePartition {
    fingerprint: String,
    rows: SourceRows,
}

#[derive(Debug, Clone)]
pub(super) struct NotePartition {
    fingerprint: String,
    rows: NoteRows,
}

#[derive(Debug, Clone)]
pub(super) struct PolicyPartition {
    fingerprint: String,
    matches: Vec<PatternMatch>,
}

#[derive(Debug, Clone, Default)]
struct ReverseDependencies {
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
        policy_fingerprints: &BTreeMap<&str, String>,
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
            .files()
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>();
        let previous_paths = previous.sources.keys().cloned().collect::<BTreeSet<_>>();
        let mut changed_source_paths = changed_files.iter().cloned().collect::<BTreeSet<_>>();
        changed_source_paths.extend(current_paths.symmetric_difference(&previous_paths).cloned());

        let mut changed_symbol_names = BTreeSet::new();
        for path in &changed_source_paths {
            if let Some(file) = vault.source_graph().file(path) {
                changed_symbol_names.extend(file.symbols.iter().map(|symbol| symbol.name.clone()));
            }
            if let Some(partition) = previous.sources.get(path) {
                changed_symbol_names
                    .extend(partition.rows.dependencies.defined_symbols.iter().cloned());
            }
        }

        let policy_changed = policy_fingerprints.len() != previous.policies.len()
            || policy_fingerprints.iter().any(|(id, fingerprint)| {
                previous
                    .policies
                    .get(*id)
                    .is_none_or(|partition| &partition.fingerprint != fingerprint)
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
                &partition.rows.dependencies,
            );
        }
        for (path, partition) in &partitions.notes {
            index.insert(
                PartitionKey::Note(path.clone()),
                &partition.rows.dependencies,
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
    pub(super) fn build(
        vault: &Vault,
        previous: Option<&Self>,
        changed_files: &[String],
        policies: &PolicyScanPlan,
    ) -> Result<Self> {
        let policy_fingerprints = policies
            .owners()
            .iter()
            .flat_map(|owner| {
                owner
                    .policies()
                    .iter()
                    .filter(|policy| policy.is_registered())
                    .map(|policy| {
                        (
                            policy.pattern_id(),
                            policy_fingerprint(policy.definition(), owner.scopes()),
                        )
                    })
            })
            .collect::<BTreeMap<_, _>>();
        let note_catalog_fingerprint = note_catalog_fingerprint(vault);
        let invalidation = InvalidationFacts::collect(
            vault,
            previous,
            changed_files,
            &policy_fingerprints,
            &note_catalog_fingerprint,
        );
        let mut partitions = Self::default();

        for file in vault.source_graph().files() {
            let fingerprint = vault
                .source_graph()
                .file_fingerprint(&file.path)
                .unwrap_or_default();
            let partition = previous
                .and_then(|previous| previous.sources.get(&file.path))
                .filter(|partition| {
                    partition.fingerprint == fingerprint
                        && !invalidation.changed_source_paths.contains(&file.path)
                        && !invalidation.affects(&PartitionKey::Source(file.path.clone()))
                })
                .cloned()
                .unwrap_or_else(|| {
                    #[cfg(test)]
                    work::record(Work::SourcePartitionBuild);
                    Arc::new(SourcePartition {
                        fingerprint: fingerprint.to_string(),
                        rows: projection::source_rows(vault, file),
                    })
                });
            partitions.sources.insert(file.path.clone(), partition);
        }

        for note in vault.notes() {
            let fingerprint = note_fingerprint(note);
            let partition = previous
                .and_then(|previous| previous.notes.get(note.rel_path()))
                .filter(|partition| {
                    partition.fingerprint == fingerprint
                        && !invalidation.affects(&PartitionKey::Note(note.rel_path().to_string()))
                })
                .cloned()
                .unwrap_or_else(|| {
                    #[cfg(test)]
                    work::record(Work::NotePartitionBuild);
                    Arc::new(NotePartition {
                        fingerprint,
                        rows: projection::note_rows(vault, note),
                    })
                });
            partitions
                .notes
                .insert(note.rel_path().to_string(), partition);
        }

        partitions.policies = build_policy_partitions(
            vault,
            previous,
            changed_files,
            policies,
            policy_fingerprints,
        )?;
        partitions.reverse_dependencies = ReverseDependencies::index(&partitions);
        partitions.note_catalog_fingerprint = note_catalog_fingerprint;
        Ok(partitions)
    }

    pub(super) fn graph(&self) -> Graph {
        let mut builder = GraphBuilder::default();
        // Every code file node precedes the Source details.
        for partition in self.sources.values() {
            builder.push_node(partition.rows.code_node.clone());
        }
        for partition in self.sources.values() {
            builder.append_rows(&partition.rows.rows);
        }
        for partition in self.notes.values() {
            builder.append_rows(&partition.rows.rows);
        }
        builder.finish()
    }

    pub(super) fn patterns(&self) -> BTreeMap<String, Vec<PatternMatch>> {
        self.policies
            .iter()
            .map(|(id, partition)| (id.clone(), partition.matches.clone()))
            .collect()
    }
}

struct PendingPolicyScan<'a> {
    pattern_id: &'a str,
    fingerprint: String,
    policy: &'a CompiledPolicy,
    paths: BTreeSet<String>,
    reused: Vec<PatternMatch>,
}

fn build_policy_partitions(
    vault: &Vault,
    previous: Option<&StatePartitions>,
    changed_files: &[String],
    policies: &PolicyScanPlan,
    mut fingerprints: BTreeMap<&str, String>,
) -> Result<BTreeMap<String, Arc<PolicyPartition>>> {
    let mut partitions = BTreeMap::new();
    let mut pending = Vec::new();
    for owner in policies.owners() {
        let matcher = GlobMatcher::from_valid_patterns(owner.scopes());
        let changed_paths = changed_files
            .iter()
            .filter(|path| matcher.is_match(path))
            .cloned()
            .collect::<BTreeSet<_>>();
        for policy in owner
            .policies()
            .iter()
            .filter(|policy| policy.is_registered())
        {
            let pattern_id = policy.pattern_id();
            let fingerprint = fingerprints.remove(pattern_id).ok_or_else(|| {
                CrivError::new(format!(
                    "registered policy `{pattern_id}` is declared twice"
                ))
            })?;
            let previous_partition =
                previous.and_then(|previous| previous.policies.get(pattern_id));
            let unchanged =
                previous_partition.filter(|partition| partition.fingerprint == fingerprint);
            if let Some(partition) = unchanged
                && changed_paths.is_empty()
            {
                partitions.insert(pattern_id.to_string(), partition.clone());
                continue;
            }
            let (paths, reused) = unchanged.map_or_else(
                || (owner.paths().clone(), Vec::new()),
                |partition| {
                    let reused = partition
                        .matches
                        .iter()
                        .filter(|matched| !changed_paths.contains(&matched.file))
                        .cloned()
                        .collect();
                    (changed_paths.clone(), reused)
                },
            );
            pending.push(PendingPolicyScan {
                pattern_id,
                fingerprint,
                policy: policy.compiled(),
                paths,
                reused,
            });
        }
    }

    let requests = pending
        .iter()
        .enumerate()
        .map(|(key, scan)| structural::PolicyScanRequest {
            key,
            policy: scan.policy,
            paths: &scan.paths,
        })
        .collect::<Vec<_>>();
    let rescanned =
        structural::find_policies_batch(vault.repository_files(), vault.source_files(), &requests)?;
    for (key, scan) in pending.into_iter().enumerate() {
        let mut matches = scan.reused;
        matches.extend(
            rescanned
                .get(&key)
                .ok_or_else(|| CrivError::new("policy scan returned no result"))?
                .iter()
                .map(pattern_match),
        );
        matches.sort_by(|left, right| {
            (&left.file, &left.range, &left.captures).cmp(&(
                &right.file,
                &right.range,
                &right.captures,
            ))
        });
        matches.dedup();
        #[cfg(test)]
        work::record(Work::PolicyPartitionBuild);
        partitions.insert(
            scan.pattern_id.to_string(),
            Arc::new(PolicyPartition {
                fingerprint: scan.fingerprint,
                matches,
            }),
        );
    }
    Ok(partitions)
}

fn pattern_match(matched: &StructuralMatch) -> PatternMatch {
    PatternMatch {
        file: matched.path.clone(),
        range: Some(matched.range.clone()),
        captures: matched.captures.clone(),
    }
}

/// The inputs of a registered policy that decide its matches.
fn policy_fingerprint(definition: &PolicyPattern, scopes: &[String]) -> String {
    let mut hasher = StableHasher::new("policy-partition");
    hash_policy(&mut hasher, definition);
    hasher.strs(scopes).finish()
}

fn hash_policy(hasher: &mut StableHasher, policy: &PolicyPattern) {
    let language = policy.language.as_ref().map(ToString::to_string);
    hasher
        .option_str(language.as_deref())
        .option_str(policy.pattern.as_deref())
        .option_str(policy.rule.as_deref());
}

fn note_fingerprint(note: &Note) -> String {
    let mut hasher = StableHasher::new("note-partition");
    hasher
        .str(note.rel_path())
        .option_str(note.id())
        .str(note.kind().as_str())
        .option_str(note.title())
        .option_str(note.status().map(DecisionStatus::as_str))
        .str(note.body())
        .strs(note.targets_symbols())
        .strs(note.targets_scope())
        .usize(note.target_pattern_refs().len());
    for pattern in note.target_pattern_refs() {
        hasher.str(&pattern.id).usize(pattern.line);
    }
    hasher
        .strs(note.target_pattern_ids())
        .usize(note.policy_patterns().len());
    for policy in note.policy_patterns() {
        hasher
            .option_str(policy.id.as_deref())
            .usize(policy.line)
            .option_str(policy.message.as_deref());
        hash_policy(&mut hasher, policy);
    }
    hasher
        .strs(note.governs())
        .strs(&note.effective_governs())
        .strs(note.supersedes())
        .strs(note.superseded_by())
        .option_str(note.frontmatter_error())
        .finish()
}

fn note_catalog_fingerprint(vault: &Vault) -> String {
    let mut hasher = StableHasher::new("note-catalog");
    hasher.usize(vault.notes().len());
    for note in vault.notes() {
        hasher
            .str(note.rel_path())
            .option_str(note.id())
            .option_str(note.title())
            .usize(note.headings().len());
        for heading in note.headings() {
            hasher.str(&heading.text).usize(heading.level);
        }
    }
    hasher.finish()
}

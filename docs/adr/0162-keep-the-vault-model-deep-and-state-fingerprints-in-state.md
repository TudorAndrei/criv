---
id: ADR-0162
kind: decision
title: Keep the Vault Model Deep and State Fingerprints in State
status: accepted
date: 2026-11-06
governs:
  - src/vault.rs
  - src/structural.rs
  - src/policy_scan.rs
  - src/state.rs
  - src/state/partitions.rs
  - src/state/projection.rs
  - src/work.rs
---

# Keep the Vault Model Deep and State Fingerprints in State

## Context

[[0065-shared-compiled-policy-scan-plans|ADR-0065]] gives `src/policy_scan.rs`
policy planning. [[0146-type-diagnostic-codes-graph-kinds-and-hash-inputs|ADR-0146]]
requires typed hash inputs. [[0072-keep-performance-observation-outside-core|ADR-0072]]
permits test-only counters.

The vault exposed its `Note` and `Vault` fields, so callers built their own
decision maps and composed the effective `governs` file list in six places.
It had four loaders with a meaningless flag combination, two test-only
loaders, and a raw status string. `src/structural.rs` imported the vault for
the policy definition and for the file list, and the vault imported
structural for the policy language, so the two modules formed a cycle.

The policy plan carried State-only data: the State pattern id, a partition
fingerprint made from `Debug` text with raw BLAKE3, and only the first State
definition error. It chose the registered declaration of a duplicated id
with pointer equality. Five modules kept their own thread-local work
counters.

## Decision

The vault model is deep. `Note` and `Vault` fields are private behind
accessors. `DecisionStatus` types the status. `Note::effective_governs` and
`Vault::governed_files` are the one effective-governs rule.
`Vault::decision` and `Vault::decisions` expose the decision map that the
vault builds once. `Vault::load` takes a `LoadOptions` value that names the
configuration and the Source observation; it is the only loader.

`src/structural.rs` owns `PolicyPattern` and `PolicyLanguage`.
`find_policies_batch` takes the repository files, the candidate paths, and
the requests. Structural does not import the vault.

The policy plan holds plain policy data: the pattern id, the definition, the
compiled matcher, a registered flag, and every registered definition
failure. A pattern id registers the first declaration of its trimmed local id
in the decision note that the ADR id resolves to. A doc note that shares an
ADR id is not an effective decision. `check::validate_vault` returns its
diagnostics with the plan it built, so a command makes one call.

State owns its partitions. It fingerprints notes and policies with
`StableHasher` over typed fields, and a Source partition with the Source
graph file fingerprint. State refuses to publish while any registered
definition fails and reports every failure. The Source index is rebuilt for
each revision; its per-row fingerprint costs more than the row.

`src/work.rs` is the one test-only work counter module.

## Consequences

A change to the note model, the governs rule, or the loader has one place.
Structural can scan without a vault, and the vault and structural modules no
longer form a cycle.

The published State document and its hash do not change. Partition
fingerprints change, so the first refresh after the upgrade rebuilds every
partition once.

A State publication with several broken registered policies names all of
them. A documentation asset with a wrong signature does not count toward the
total bound of [[0131-publish-verified-documentation-assets-for-native-previews|ADR-0131]].

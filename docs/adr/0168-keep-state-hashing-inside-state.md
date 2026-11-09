---
id: ADR-0168
kind: decision
title: Keep State Hashing Inside State
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0146
governs:
  - src/diagnostic.rs
  - src/state/stable_hash.rs
  - src/state.rs
  - src/state/projection.rs
  - src/state/partitions.rs
  - src/source/graph.rs
  - crates/criv-state-wire/src/lib.rs
  - fixtures/state/criv.state.v1.json
---

# Keep State Hashing Inside State

## Context

[[0146-type-diagnostic-codes-graph-kinds-and-hash-inputs|ADR-0146]] types
diagnostic codes and graph kinds, and hashes every published graph value
through `StableHasher` in `src/stable_hash.rs`. Only the State projection and
the State partitions use that type, but it was a crate-root module.

## Decision

Retain the complete decision of ADR-0146. `StableHasher` is the private
`src/state/stable_hash.rs` child of State. Code outside State does not hash
published graph values or partition fingerprints.

The State contract test keeps the pinned fixture and the pinned snapshot hash
as a deliberate compatibility check.

## Consequences

A change to the hash encoding has one owner, and the crate root has one module
fewer. The hash values and the State document do not change.

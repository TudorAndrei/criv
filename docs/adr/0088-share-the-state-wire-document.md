---
id: ADR-0088
kind: decision
title: Share the State Wire Document
status: accepted
date: 2026-08-08
supersedes:
  - ADR-0071
governs:
  - Cargo.toml
  - criv.toml
  - crates/criv-state-wire/Cargo.toml
  - crates/criv-state-wire/src/lib.rs
  - crates/criv-wasm/Cargo.toml
  - crates/criv-wasm/src/lib.rs
  - src/state.rs
  - src/snapshots.rs
  - src/init/templates.rs
  - scripts/performance/Cargo.toml
  - scripts/performance/src/bin/criv-state-storage-baseline.rs
  - scripts/performance/state-store-prototype/Cargo.toml
  - scripts/performance/state-store-prototype/src/lib.rs
  - extensions/vscode-criv/src/wasm.ts
  - extensions/vscode-criv/src/stateStore.ts
  - extensions/vscode-criv/src/languageFeatures.ts
  - .obsidian/plugins/criv/src/wasm.ts
  - .obsidian/plugins/criv/src/core.ts
  - .obsidian/plugins/criv/src/main.ts
---

# Share the State Wire Document

## Context

[[0071-make-wasm-editor-projections-canonical|ADR-0071]] made Wasm the only
editor-local State validation and projection implementation. The native State
publisher, snapshot store, and Wasm library still declared the State schema
identity or serialized row types in separate modules. A wire change could
therefore pass one consumer and fail another consumer.

The CLI also has incremental State partitions. These partitions support fast
refresh work, but they are not part of the published State document. Shared
wire ownership must not make these private types part of the consumer
contract.

[[0081-require-material-state-store-performance-gains|ADR-0081]] sets strict
gates for a State format or storage change. Shared Rust types do not give
authority to change the JSON format or to add repeated serialization and
decoding work.

## Decision

Add one `criv-state-wire` Rust library. It is the only module that declares the
current State schema identity, the complete serialized State document, and its
graph, pattern-match, source-index, and architecture row types.

The native State publisher builds this shared document from private State
partitions and serializes the shared document for both the latest State and
the content-addressed snapshot. The initializer uses the shared empty document.
The snapshot store and performance harnesses use the shared schema identity
when they validate a document. Incremental partition keys, dependency facts,
fingerprints, and allocation reuse stay private to the CLI.

Wasm keeps editor-local validation and projection preparation. It reads the
schema from the editor-provided envelope, rejects invalid JSON and unsupported
schema identities with distinct errors, decodes the shared wire document, and
then prepares summaries, safe sources, graph nodes, lookup data, and selector
data. TypeScript adapters do not parse State or provide fallback projection,
lookup, matching, scoring, or ranking behavior.

The compiled Wasm package remains a required editor asset. A missing, corrupt,
or incompatible runtime produces a stable visible failure. Editors still own
host I/O and notices. They keep one loaded revision per workspace and dispose
it as required by
[[0083-own-one-loaded-state-revision-per-editor-workspace|ADR-0083]].

This ownership change keeps the `criv.state.v1` JSON document unchanged. It
does not select a new format or store. Any later format or storage change must
pass every correctness and performance gate in ADR-0081. Native publication
still serializes one complete document per revision, and Wasm still prepares
one loaded revision for repeated editor operations.

## Consequences

Native publication, snapshot validation, and Wasm preparation cannot drift to
different schema names or row shapes. Contract tests can use one Rust document
type across both consumers.

The workspace has one more small Rust crate, and its consumers add workspace
path dependencies. This crate has a narrow wire interface and no file, editor,
refresh, or projection behavior.

Wasm still owns the validation result that editors see. The shared document
type does not move trust to TypeScript and does not weaken runtime failure
reporting.

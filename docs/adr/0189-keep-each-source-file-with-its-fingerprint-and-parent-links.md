---
id: ADR-0189
kind: decision
title: Keep Each Source File With Its Fingerprint and Parent Links
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0183
governs:
  - src/source/graph.rs
  - src/source/graph/extract.rs
  - src/source/graph/elixir.rs
  - src/source/paths.rs
  - src/state.rs
  - src/state/partitions.rs
  - src/state/projection.rs
  - src/watch/refresh.rs
  - crates/criv-state-wire/src/source_identity.rs
  - crates/criv-wasm/src/source.rs
---

# Keep Each Source File With Its Fingerprint and Parent Links

## Context

[[0183-derive-source-graph-indexes-from-its-files|ADR-0183]] made
`SourceGraph::from_files` the one constructor and stored the parsed files and
their fingerprints in the cache. The fingerprints were a second map beside the
files, so a reader had to look up each file in both maps, and State partitions
used an empty fingerprint when the second map had no entry.

ADR-0183 also said that the State projection writes `contains` edges only from
`parent_id`. The Elixir extractor did not set `parent_id` on callables and
callbacks, so the projection still searched the file for a symbol with the same
owner and no arity. `StatePartitions::build` and the policy partitions still
took the changed files as an argument that every caller read from the Vault.

## Decision

Each `SourceFile` holds the BLAKE3 fingerprint of the bytes it was parsed from.
`SourceGraph::from_files` takes the files and the changed paths. It builds the
symbol index and the Elixir relationship index. The cache stores only the
files; its schema is `criv.source-graph/6`. A cache of an earlier schema is
ignored and rebuilt.

The graph and the Elixir extractor use the shared `ElixirOwner`. A symbol
records `parent_id`, the symbol in the same file that contains it. The
extractor sets it from the enclosing class or Elixir module. For a container
such as a Rust `impl`, it names the one type symbol in the file with that name.
The State projection writes the `contains` edge from `parent_id` and resolves
no name or owner.

State partitions read the changed files from the Vault Source observation.
Tests reach incremental State through a second Vault load, which compares
against the published graph cache.

Each language row also holds its call node kinds, the node kinds that make a
variable a function value, and its entry-point name. A grammar that does not
load is an error. `source::read_selected` and `source::read_selected_text` are
the reads of a selected file. Graph code, the Wasm lookup, and symbol
resolution read and write identity text through `SourceIdentity`.

## Consequences

The first refresh after an upgrade parses every Source file once.

A partition can no longer compare against a missing fingerprint. An Elixir
module that is declared twice in one file contains only its own callables.

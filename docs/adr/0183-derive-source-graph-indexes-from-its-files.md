---
id: ADR-0183
kind: decision
title: Derive Source Graph Indexes From Its Files
status: accepted
date: 2026-11-06
governs:
  - src/source/graph.rs
  - src/source/graph/extract.rs
  - src/source/graph/elixir.rs
  - src/source/paths.rs
  - src/state.rs
  - src/state/projection.rs
  - src/watch/refresh.rs
  - crates/criv-state-wire/src/source_identity.rs
  - crates/criv-wasm/src/source.rs
---

# Derive Source Graph Indexes From Its Files

## Context

The Source graph cache stored the parsed files, their fingerprints, and the
symbol index. The Elixir relationship index was not stored, so each loader
had to rebuild it after deserialization, in the correct order. The graph kept
its own owner type with the same shape as the shared `ElixirOwner` of
[[0130-share-structured-source-identity-text|ADR-0130]].

A symbol recorded its parent only by name. The State projection found the
parent again by resolving `path#name`, so a file with two symbols of that name
made the lookup ambiguous, and the `contains` edge from the class to its
member was lost.

`write_state` took the list of changed files as an argument. Production passed
the list that the Vault Source observation already holds; only tests passed
other lists. The language table of
[[0161-describe-source-languages-in-one-table|ADR-0161]] did not hold the call
node kinds, the function-value node kinds, or the entry-point name, and a
grammar that failed to load gave an empty file. Three helpers read a selected
file, and the Source and the editor split identity text on `#` by hand.

## Decision

`SourceGraph::from_files` is the one constructor. It takes the parsed files,
their fingerprints, and the changed paths, and builds the symbol index and the
Elixir relationship index. The cache stores only the files and the
fingerprints; its schema is `criv.source-graph/5`. A cache of an earlier
schema is ignored and rebuilt.

The graph and the Elixir extractor use the shared `ElixirOwner`. A symbol
records `parent_id`, the symbol in the same file that contains it. The
extractor sets it from the enclosing class. For a container such as a Rust
`impl`, it names the one type symbol in the file with that name. The State
projection writes the `contains` edge from `parent_id` and resolves no name.

`write_state` reads the changed files from the Vault Source observation. Tests
reach incremental State through a second Vault load, which compares against
the published graph cache.

Each language row also holds its call node kinds, the node kinds that make a
variable a function value, and its entry-point name. A grammar that does not
load is an error.

`source::read_selected` and `source::read_selected_text` are the reads of a
selected file. Graph code, the Wasm lookup, and symbol resolution read and
write identity text through `SourceIdentity`. The Elixir callable name and
arity are public on `ElixirSelector`.

## Consequences

The first refresh after an upgrade parses every Source file once.

A member keeps its `contains` edge when another symbol in its file has the
name of its class.

---
id: ADR-0160
kind: decision
title: Own the Published State Grammar in the Wire Crate
status: accepted
date: 2026-11-06
governs:
  - crates/criv-state-wire/src/**/*.rs
  - crates/criv-wasm/src/source.rs
  - crates/criv-wasm/src/asset.rs
  - src/state/projection.rs
  - src/query.rs
---

# Own the Published State Grammar in the Wire Crate

## Context

[[0088-share-the-state-wire-document|ADR-0088]] made `criv-state-wire` the
only owner of the State schema identity and the serialized row types.
[[0130-share-structured-source-identity-text|ADR-0130]] added the Source
identity text to that crate. The text inside the rows still had several
owners:

- The native State projection wrote the node id prefixes `note:`, `code:`,
  `symbol:`, `pattern:`, `import:`, `external-call:`, `external-module:`, and
  `dynamic-call:`, and the `#L` line locations. Wasm parsed `symbol:`, `code:`,
  and `#L` again by hand.
- `criv query diff` and Wasm each formatted the edge row `{from}:{kind}:{to}`.
- The snapshot hash rule (BLAKE3 over the JSON without its final newline) was
  written in the publisher, the snapshot store, and test helpers. The store
  accepted only lowercase hex names, publication accepted any case, and Wasm
  had a third check for asset hashes.
- The asset size limits and the extension to media type table were copied in
  the vault, discovery, and Wasm.
- Each `.criv/` path was a string literal in the module that used it.
- The selector aliases `name`, `name/arity`, and `Module.name/arity` were
  built by the native Source graph and again by Wasm.

## Decision

`criv-state-wire` owns every piece of published State text that the CLI writes
and Wasm reads. This refines the wire-ownership rule of ADR-0088; it does not
change the `criv.state.v1` document.

- `node_id` has one constructor per node id kind, one parser, and the
  `#L` location forms. Wasm reads source targets and line ranges through it.
- `Edge::row` is the edge row text. `GraphRowsView` reads node ids and edge
  rows without the kind types, so `criv query diff` still compares revisions
  that hold a kind this build does not know, as ADR-0146 requires.
- `content_hash` holds the BLAKE3 content hash, the snapshot hash of a
  published document, and the one validity check: 64 lowercase hex digits.
- `asset` holds the asset size limits and the exact lowercase extension table
  of [[0133-use-exact-lowercase-documentation-asset-extensions|ADR-0133]].
- `layout` names every path under `.criv/`.
- `source_identity::compatibility_aliases` is the one alias rule. The native
  Source graph and Wasm call it; Wasm still owns lookup indexes, ambiguity,
  and ranking as ADR-0130 states.

The CLI configuration keeps the one normalizer for configured paths. Wasm keeps
its own check for published paths, because it defends against a hostile State
document and does not accept the root `.`.

## Consequences

A change to an id prefix, a location form, a hash rule, an asset type, or a
`.criv/` path has one place to edit, and both Rust consumers follow it.

The wire crate depends on `blake3`. `serde_json` is only a test dependency.

Publication now treats only lowercase hex snapshot names as managed, as the
snapshot store does. Wasm drops an asset row whose hash is not lowercase hex.
The native publisher never writes such names or hashes.

Wasm now resolves a file basename with a symbol fragment, such as
`lib.rs#run`, as the CLI does.

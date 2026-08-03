---
id: ADR-0071
kind: decision
title: Make Wasm Editor Projections Canonical
status: accepted
date: 2026-08-03
governs:
  - crates/criv-wasm/src/lib.rs
  - extensions/vscode-criv/src/wasm.ts
  - extensions/vscode-criv/src/stateStore.ts
  - extensions/vscode-criv/src/languageFeatures.ts
  - .obsidian/plugins/criv/src/wasm.ts
  - .obsidian/plugins/criv/src/core.ts
  - .obsidian/plugins/criv/src/main.ts
---

# Make Wasm Editor Projections Canonical

## Context

The Obsidian and VS Code companions consume the same generated `criv.state.v0`
document but project it through several independent implementations.
`crates/criv-wasm/src/lib.rs` validates and summarizes state, enumerates source
entries and graph nodes, looks up graph nodes, and ranks source-selector
suggestions. The TypeScript bridges repeat some or all of that parsing and
ranking as fallbacks.

Both bridges currently convert a failed dynamic Wasm import to `null`. A
missing or corrupt packaged runtime therefore changes projection and ranking
behavior silently. Users cannot distinguish canonical Wasm results from a
fallback, and changes to the Rust implementation require parity work across
multiple TypeScript copies.

[[0035-vscode-compatible-companion-extension|ADR-0035]] already keeps editor
companions on the consumer side of the generated-state boundary. This decision
settles the implementation boundary inside those consumers.

## Decision

`crates/criv-wasm/src/lib.rs` is the sole implementation of editor-local state
validation, summaries, source entries, graph-node projections, graph-node
lookup, and source-selector ranking. Both TypeScript bridges may define host
types and adapt returned values, but they must not parse generated state or
implement fallback projection, lookup, matching, scoring, or ranking behavior.

Treat the compiled Wasm package as a required companion runtime asset. Cache
one load attempt per extension or plugin activation. A missing, corrupt, or
incompatible module must reject with a stable descriptive runtime error rather
than resolve to a fallback. Invalid `criv.state.v0` input remains a distinct
state-validation error returned by the canonical exports.

State stores, status views, and suggestion callers must translate runtime-load
failure into a visible editor status or warning. They may return no projection
or suggestions while the runtime is unavailable, but they must not substitute
different semantics. Repeated reads and completion requests must not emit
duplicate notifications for the same cached load failure; recovery occurs
after rebuilding the runtime and reloading the companion.

Companion production and packaging commands must build the Wasm target and
include its generated JavaScript and `.wasm` files. Tests must exercise the
compiled canonical exports, reject unsupported state schemas, and cover
missing or corrupt runtime assets explicitly. Ranking parity follows from both
companions calling the same export rather than comparing independent scorers.

## Consequences

Both editors expose the same state projections and selector order by
construction. Rust tests become the focused specification for projection and
ranking behavior, while bridge tests cover loading, type adaptation, host
status, and warning behavior.

A broken or omitted Wasm package now disables affected editor features visibly
instead of preserving a partial experience. That failure is sharper, but it is
diagnosable and cannot be mistaken for canonical behavior. Companion builds
and packages must therefore treat Wasm output as required rather than an
optional optimization.

The Rust CLI remains authoritative for generating and validating vault state.
This decision only makes `criv-wasm` authoritative for editor-local projections
over that generated state.

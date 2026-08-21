---
id: ADR-0126
kind: decision
title: Own One Source State Per Refresh
status: accepted
date: 2026-08-21
governs:
  - src/lib.rs
  - src/discovery/mod.rs
  - src/refresh.rs
  - src/refresh/tests.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/vault.rs
---

# Own One Source State Per Refresh

## Context

[[0115-single-read-source-build|ADR-0115]] requires one Source build to make
the Source catalog and parsed graph from the same file reads. The current
`SourceBuild` value first satisfies that rule, but its interface then exposes
the catalog and graph build as separate values. `src/vault.rs` stores those
values in separate fields and later reconstructs a `SourceBuild` from them.

`src/refresh.rs` also owns three related lifecycle values: an optional graph
cache seed, an optional Source build, and a previous Vault that stores another
catalog and graph build. The code keeps these values correlated by convention.
The type interface permits a catalog from one observation to be paired with a
graph from another observation.

[[0105-owner-scoped-rust-module-layout|ADR-0105]] makes `src/source.rs` the
Source interface. Catalog lookup, parsing, graph construction, and cache reuse
are private Source implementations. Their completed state must therefore have
one owner and one interface.

## Decision

Define one crate-local `SourceState` type at the `src/source.rs` interface.
It owns the stable Source path catalog, parsed graph, changed-file markers, and
graph-cache disposition for one completed Source observation. Its invariants
are:

1. Catalog paths are the graph's stable sorted file paths.
2. Catalog entries, graph data, changed-file markers, and cache disposition
   come from the same observation.
3. A disabled Source state has an empty catalog and graph together.

Keep catalog lookup in `src/source/catalog.rs` and graph parsing and cache
implementation in `src/source/graph.rs`. Make `SourceCatalog`,
`SourceGraphBuild`, cache loading, cache disposition, and part-level
constructors private Source implementation details. Do not expose
`from_parts`, `into_parts`, or another caller interface that can assemble a
Source state from independent values.

Use this external Source interface:

- `SourceState::refresh(root, config, previous)` performs candidate discovery,
  one-read classification and parsing, incremental graph reuse, catalog
  derivation, and cache publication. When `previous` is absent, it loads a
  compatible graph cache as an internal seed. When Source indexing is
  disabled, it returns one disabled state without a Source walk.
- `SourceState::reuse_for_docs()` returns the same catalog and graph for a
  docs-only refresh, clears changed-file markers, and performs no Source walk,
  file read, parse, or cache publication.
- Read-only methods expose the paths, entries, parsed graph, changed files, and
  partial-path resolution that callers need. They do not expose build parts or
  cache state.

Store one `SourceState` field in `Vault`. A normal Vault load obtains one
completed Source state from the Source interface. A docs-only Vault load uses
the disabled state. An incremental Vault load accepts one completed
`SourceState`; it cannot accept a catalog and graph separately. Keep the
existing Vault query interface as a small delegate so State, Query, Check,
Enforcement, and structural scanning do not learn Source implementation types.

Make `RefreshSession` own its configuration and last successful
`RefreshResult`. Remove its separate graph seed and Source build fields. The
previous result's Vault is the only in-memory owner of the last successful
Source state.

- An initial or Source-change refresh calls `SourceState::refresh` with the
  previous successful Source state when one exists.
- A docs-only refresh calls `reuse_for_docs` on the previous successful Source
  state.
- The candidate Source state becomes authoritative only when Vault validation
  and State publication complete. Any failure leaves the previous result and
  its Source state unchanged.

Keep `.criv/source-graph.json`, its schema, its recovery behavior, and its
publication timing unchanged. Keep the single-read rule, binary and UTF-8
rules, stable order, source target results, CLI output, serialized State,
editor behavior, one-shot and live parity, and watch recovery rules from
ADR-0115 and [[0092-transactional-live-watch-generations|ADR-0092]].

This module is local-substitutable. Test it through the `SourceState`
interface with temporary repositories and the real confined filesystem
implementation. Do not add a filesystem port, trait, or mock only for this
refactor; there is one production adapter and temporary repositories already
provide the required test seam.

The Source state does not own stable selector text, selector decoding, or
Elixir-specific graph meaning. Those rules remain coordinated follow-up
decisions. This change must preserve their current serialized text and graph
behavior.

## Migration

1. Add `SourceState` to `src/source.rs` and move Source build, cache seed,
   reuse, publication, and read-only query orchestration behind it.
2. Test the completed-state invariants, disabled behavior, warm cache reuse,
   docs-only reuse, Source-change rebuild, and failed-refresh retention through
   the Source and Refresh interfaces.
3. Replace the two Source fields in Vault with one `SourceState` and replace
   split injection with one completed-state input.
4. Remove the separate graph seed and Source build from `RefreshSession`. Use
   only the Source state in the previous successful Vault.
5. Remove crate-level exports and constructors for `SourceCatalog`,
   `SourceGraphBuild`, `SourceBuild`, and cache loading. Keep child tests only
   for private parser and lookup details that the Source interface cannot
   observe.
6. Update the Code architecture map to show `criv::source` as the state and
   lifecycle interface, with catalog and graph as private children. Run the
   full Rust, vault, and architecture validation gates.

## Consequences

Callers can no longer create or retain a split Source observation. The Source
interface has more depth: one value hides discovery, one-read construction,
incremental reuse, cache publication, catalog lookup, and graph access.

Refresh ownership is smaller. The last successful Vault is the only
authoritative in-memory Source state, and candidate work cannot replace it
before the complete refresh succeeds.

Deleting the Source module would spread catalog derivation, graph reuse,
cache rules, disabled behavior, and refresh correlation back across Vault and
Refresh. The module therefore passes the deletion test.

## Alternatives Considered

### Keep SourceBuild but split it inside Vault

Rejected. A temporary coherent value does not protect later callers after its
parts become independent fields.

### Let Vault own catalog and graph correlation

Rejected. Catalog and graph construction are Source implementation rules.
Putting their invariant in Vault would weaken the Source interface and make
Refresh depend on Vault internals.

### Expose several Source lifecycle types

Rejected. Public seed, candidate, built, and published types would make callers
learn cache and ordering details. One completed state plus private internal
states gives a smaller interface.

### Add a Source filesystem port

Rejected. There is one local filesystem implementation. A second hypothetical
interface would add indirection without another adapter.

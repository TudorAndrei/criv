---
id: ADR-0105
kind: decision
title: Use Owner-Scoped Rust Module Layout
status: accepted
date: 2026-08-13
governs:
  - src/source.rs
  - src/source/**/*.rs
  - src/state.rs
  - src/state/**/*.rs
  - crates/criv-wasm/src/**/*.rs
  - crates/criv-state-wire/src/lib.rs
---

# Use Owner-Scoped Rust Module Layout

## Context

The CLI crate used root files such as `source_graph.rs`, `source_index.rs`, and
`source_paths.rs` for one Source owner. State publication and snapshot work
also appeared as unrelated root modules. The file names suggested a hierarchy,
but the Rust module tree did not contain that hierarchy.

Cargo defines `src/lib.rs` and `src/main.rs` as crate roots. The modern Rust
module layout keeps an owner interface in `owner.rs` and child modules in
`owner/*.rs`. The older `owner/mod.rs` form is supported, but it gives many
unrelated files the same name.

File length alone does not define an owner. Compound names such as
`install_editor.rs` and `policy_scan.rs` can identify one complete concern.

## Decision

Use Cargo package roots and the modern file-plus-directory module layout.
Create a directory only when several files implement one owner scope.

`src/source.rs` is the Source interface. Its private child modules own graph
construction, indexing, and safe source paths. `source_reconcile.rs` stays in
the Governance scope because it changes ADR source references and does not
implement Source intelligence.

`src/state.rs` is the State interface. Its private child modules own
publication and snapshots. Callers use the State interface and do not import
those child modules.

`criv-wasm` keeps `LoadedState` and all Wasm exports in `src/lib.rs`. Private
child modules own State decoding, initial projections, source lookup and
selector ranking, and LikeC4 projection. `criv-state-wire` stays in one
`src/lib.rs` while it has one small wire-contract responsibility.

Keep a compound root filename when it names one complete concern. Do not add a
barrel or forwarding module only to preserve an old internal path. Child
modules stay private unless a caller needs the child interface.

## Consequences

The file tree and module tree show the same owners. Callers learn one interface
for Source and one interface for State. Internal file moves do not add a second
interface.

The change does not alter CLI behavior, configuration behavior, serialized
State, or Wasm exports. New owner scopes use the same rule.

## Alternatives Considered

### Keep flat prefixed files

Rejected. Prefixes imitate scope without creating a module interface.

### Use `mod.rs` for each owner

Rejected. The modern file-plus-directory form gives each interface a distinct
filename and follows the current Rust module guide.

### Move every compound filename

Rejected. An underscore does not prove that a file contains several owners.

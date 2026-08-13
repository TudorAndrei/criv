---
id: ADR-0006
kind: decision
title: Fff Source Index And Incremental Watch
status: accepted
date: 2026-05-13
governs:
  - src/source/index.rs
  - src/watch.rs
  - src/vault.rs
  - src/state.rs
  - src/source/graph.rs
---

# Fff Source Index And Incremental Watch

## Context

The spec called for fff-backed path search, grep, partial-path resolution, and
source-side watcher integration. The May 13 implementation review found that
`criv watch` still owned all notify events directly and rebuilt fresh vault
state after each change.

## Decision

Wrap `fff-search` behind `src/source_index.rs` and use it for fuzzy file
search, grep, source file enumeration, and partial-path resolution. Let fff own
source-tree watching where possible, while `src/watch.rs` continues to watch
docs and coordinates rebuilds.

Carry previous graph and pattern-match state through watch rebuilds. Reuse
unchanged parsed source files and preserve unchanged pattern match results in
`src/source_graph.rs` and `src/state.rs`.

## Consequences

Watch mode becomes closer to the intended incremental architecture: source
indexing, source graph parsing, and pattern scanning can avoid full recomputation
for unchanged files.

The public criv behavior stays the same, but the internal rebuild path now
depends on stable file fingerprints and careful invalidation of deleted or
changed files.

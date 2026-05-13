---
id: ADR-0007
kind: decision
title: Content Addressed State And Diffing
status: accepted
date: 2026-05-12
governs:
  - src/state.rs
  - src/query.rs
  - src/watch.rs
---

# Content Addressed State And Diffing

## Context

The graph is useful only if criv can compare it across time. The original spec
called for content-addressed snapshots under `.criv/snapshots/` and diff queries
over graph nodes and edges. Early state writing existed, but snapshot identity
needed stable hashes and git-ref resolution.

## Decision

Make [[src/state.rs]] own `.criv/state.json`, content-addressed local snapshots,
node hashes, edge hashes, and graph root hashes. `criv watch --once` and watch
rebuilds write the latest state and snapshot pointer.

Make [[src/query.rs]] resolve `query diff <a> <b>` against local snapshots first
and then git refs when a snapshot hash is not found.

## Consequences

Local development can compare graph state without a server or database. CI and
release workflows can use the same diff mechanism against snapshots or commits.

The state schema must remain stable enough for the Obsidian plugin and future
diff consumers, so additive changes are preferred over incompatible rewrites.

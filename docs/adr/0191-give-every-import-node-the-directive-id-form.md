---
id: ADR-0191
kind: decision
title: Give Every Import Node the Directive Id Form
status: accepted
date: 2026-11-06
governs:
  - src/source/graph.rs
  - src/source/graph/extract.rs
  - src/state/projection.rs
  - crates/criv-state-wire/src/node_id.rs
---

# Give Every Import Node the Directive Id Form

## Context

State published two id forms for import nodes. An Elixir directive used
`import:<path>:<kind>:<module>:<site>`. An import of another language had the
graph kind `Legacy` and used `import:<path>:<module>`, with no site. The
projection and the wire crate each kept a branch for the second form, and two
imports of one module in one file shared one node.

## Decision

The graph has no `Legacy` directive kind. An import of a language without
Elixir directive scoping has the kind `import` and the byte offset of its
syntax node as its site. Every import node id has the form
`import:<path>:<kind>:<module>:<site>`, built by `node_id::directive`.

## Consequences

This is a breaking change of the published State: the import node ids of
non-Elixir files change, and a diff across the upgrade shows them as removed
and added. The `contains` and `imports` edges keep their meaning. A Source
graph cache with a `Legacy` import does not decode and is rebuilt.

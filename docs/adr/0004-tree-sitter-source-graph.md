---
id: ADR-0004
kind: decision
title: Tree Sitter Source Graph
status: accepted
date: 2026-05-12
governs:
  - src/source_graph.rs
  - src/state.rs
  - src/query.rs
---

# Tree Sitter Source Graph

## Context

The original spec required a structural source graph, but the first
implementation used conservative lexical parsing to establish command behavior.
That was enough for smoke tests, but it could not reliably extract symbols,
ranges, containment, imports, and calls across Rust, TypeScript, JavaScript,
Python, and Go.

## Decision

Make `src/source_graph.rs` the source graph boundary and back it with
tree-sitter grammars for the supported languages. Keep the public graph shape
stable for `src/query.rs` and `src/state.rs`, while improving extraction of
files, imports, symbols, ranges, containment, exported/public symbols, and call
edges.

Conservative fallback behavior remains acceptable when parsing fails, but the
primary implementation should be grammar-backed.

## Consequences

Queries such as callers, callees, attack surface, and undocumented-code coverage
can operate on symbol-level data instead of file-only approximations.

Tree-sitter language selection remains grammar/config driven. MIME detection is
only used to classify files for text handling and previews, not to decide parser
semantics.

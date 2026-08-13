---
id: ADR-0034
kind: decision
title: AST-aware Source Selectors
status: accepted
date: 2026-06-21
supersedes:
  - ADR-0033
governs:
  - src/source/graph.rs
  - src/vault.rs
  - src/check.rs
  - src/c4.rs
  - src/state.rs
  - src/query.rs
---

# AST-aware Source Selectors

## Context

[[0033-typed-wikilink-source-references|ADR-0033]] chose typed Wikilinks such as
`[[source:src/check.rs#validate_links]]` for source references. That fixed the
immediate question of distinguishing source links from note links, but it still
left two problems coupled together:

- Wikilinks are the document-reference format in Obsidian.
- Plain source anchors such as `path#name` are not collision-free when multiple
  AST symbols in one file share a name.

criv also has source targets outside note prose: `governs`, `targets.symbols`,
C4 `criv:source` annotations, source graph state IDs, query output, generated
C4 code diagrams, and check diagnostics. Those references should share one
source-target identity instead of each surface inventing its own spelling.

## Decision

Use Wikilinks for document and note references. Note references should continue
to follow the file-backed Wikilink convention from
[[0020-portable-note-wikilinks|ADR-0020]].

Use AST-aware source selectors for code and source references wherever criv can
resolve them. This applies to source governance, source anchors, source graph
IDs, source query targets, generated architecture references, and any future
source reference syntax in note prose.

The selector grammar should be AST-native and semantic rather than a raw
tree-sitter node path or a line-based target. It should be derived from source
concepts such as file path, symbol kind, parent or containing symbol, and
qualified name. File-level and glob-level scopes may remain path-shaped because
they do not identify an AST symbol. Line and range data may remain diagnostic
or display metadata, but must not be part of canonical source identity.

criv currently uses direct tree-sitter traversal for the source graph and
`ast-grep-core` for structural search and policy enforcement. The selector work
should evaluate whether ast-grep can help define or validate AST-native
selectors, but it should not migrate source graph extraction wholesale unless
that spike proves ast-grep simplifies extraction without losing imports, calls,
containment, interface signatures, or incremental behavior.

Existing `path#name`, bare source Wikilinks, and ADR-0033 `source:` Wikilinks
need a compatibility path. They may continue to resolve during migration, but
checks should guide new code and docs toward AST-aware selectors for code
targets and Wikilinks only for document targets.

## Consequences

Source identity becomes a source graph concern instead of an Obsidian link
convention. That lets governance, C4 anchors, generated state, and prose source
references converge on one collision-resistant model.

The exact selector grammar must be designed and tested before implementation.
It should favor readable semantic selectors over parser-internal paths and must
avoid line-based selectors as canonical identity.

ADR-0033 is superseded for the canonical source-reference format. Its
compatibility concern still matters, but typed source Wikilinks are no longer
the target end state for code references.

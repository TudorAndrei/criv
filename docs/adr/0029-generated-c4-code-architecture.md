---
id: ADR-0029
kind: decision
title: Generated C4 Code Architecture
status: accepted
date: 2026-06-19
supersedes:
  - ADR-0028
governs:
  - criv.toml
  - src/*.rs
---

# Generated C4 Code Architecture

## Context

[[0028-c4-standard-alignment-for-mermaid-diagrams|ADR-0028]] kept
`c4-code <path-glob>` as a focused query because ad hoc Code-level diagrams can
become unreadable when they cover an entire repository. That remains useful for
CLI exploration, but the repository architecture documentation now needs a
stable Level 4 Code view that is faithful to the current implementation.

criv already indexes source files and builds a source graph from
`[source].roots` when `[index].source = true`, as decided by
[[0004-tree-sitter-source-graph|ADR-0004]] and
[[0006-fff-source-index-and-incremental-watch|ADR-0006]]. Requiring a second
architecture-specific source glob would make the generated architecture view
look like a separate indexing system even though the data already exists in
`Vault::source_graph()`.

The generated architecture file also changes the meaning of `watch --once`: it
may write a committed documentation note in addition to `.criv/state.json` and
content-addressed snapshots. That behavior needs to be explicit and opt-in.

## Decision

Add opt-in generated C4 Level 4 architecture through `[architecture.code]` in
`criv.toml`. The configuration accepts only the generated note path and title:

```toml
[architecture.code]
output = "docs/architecture/04-code.md"
title = "Code diagram for criv"
```

Do not add an architecture `glob` setting. The generated Code architecture uses
all code already present in the loaded vault source graph. Source scope remains
controlled by the existing `[source].roots`, `[source].exclude`, and
`[index].source` settings.

The generated note is a regular Markdown vault note with stable frontmatter,
a generated-content notice, and a Mermaid `classDiagram` block. Mermaid C4 has
no Code-level syntax, so criv continues to use `classDiagram` for Level 4 Code
views.

Extract the existing C4 Code diagram logic from `src/query.rs` into a shared
generator. `criv query c4-code <path-glob>` must keep its current scoped query
behavior, including valid Mermaid no-match output. The architecture writer uses
the same generator through an all-indexed-source entry point so query output and
generated documentation do not drift.

Run generation from the existing watch rebuild path after the vault has loaded
and built `Vault::source_graph()`. Write the generated file only when content
changes. If the file changes, reload the vault before validation and state
serialization so the generated architecture note is validated and included in
`.criv/state.json` during the same run.

## Consequences

Generated Code architecture is faithful to criv's indexed implementation rather
than to a hand-picked diagram scope. Users who want less code in the generated
view should change source indexing configuration or wait for future generated
subviews rather than rely on a hidden architecture-only filter.

Whole-repository Code diagrams can be dense. The tradeoff is intentional for
the repository-level architecture file: deterministic generated output is more
valuable than a manually scoped view that silently omits indexed code.

Automatic documentation writes remain opt-in. Repositories that do not declare
`[architecture.code]` keep the current behavior where watch rebuilds state but
does not create or update architecture notes.

The `.criv/state.json` schema does not change. The generated Code architecture
note is ordinary vault content, so existing validation, state generation,
queries, hooks, and CI behavior continue to apply.

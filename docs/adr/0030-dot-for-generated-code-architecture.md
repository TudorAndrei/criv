---
id: ADR-0030
kind: decision
title: DOT For Generated Code Architecture
status: accepted
date: 2026-06-19
supersedes:
  - ADR-0029
governs:
  - criv.toml
  - src/*.rs
---

# DOT For Generated Code Architecture

## Context

[[0029-generated-c4-code-architecture|ADR-0029]] made the generated C4 Level 4
Code architecture faithful to all indexed source graph code and kept Mermaid
`classDiagram` as the generated note format. That satisfied the source-scope
decision, but validation against the current implementation showed that one
exhaustive Mermaid diagram is not a reliable rendering target.

The generated all-source diagram contains hundreds of code nodes and hundreds of
call relationships. Mermaid CLI can render smaller prefixes and individual
relationships from the generated output, but it fails on the full graph during
layout with an internal `Cannot set properties of undefined (setting 'order')`
error. This points to renderer scale/shape limits rather than malformed source
graph data.

The authored C4 System Context, Container, and Component diagrams remain small,
human-scale Markdown-native diagrams. The scoped `criv query c4-code
<path-glob>` command also remains useful as a focused Mermaid class diagram for
local exploration.

## Decision

Supersede ADR-0029 only for the generated all-indexed Code architecture
notation.

Keep Mermaid for authored `C4Context`, `C4Container`, and `C4Component` vault
content, and keep `criv query c4-code <path-glob>` as a scoped Mermaid
`classDiagram` query.

Generate `docs/architecture/04-code.md` as a fenced Graphviz DOT graph instead
of a Mermaid `classDiagram`. The DOT graph still uses all code present in
`Vault::source_graph()`, with source scope governed only by `[source].roots`,
`[source].exclude`, and `[index].source`.

Use source symbol IDs as DOT node IDs and source paths plus symbol names as
labels. This preserves duplicate function or method names that exist in
different files, making the generated architecture more faithful than the
previous Mermaid class-name-only projection.

Keep `[architecture.code]` opt-in and keep its configuration surface unchanged:

```toml
[architecture.code]
output = "docs/architecture/04-code.md"
title = "Code diagram for criv"
```

Do not add a renderer dependency to criv. The generated DOT is deterministic
text in the vault. Rendering through Graphviz `dot` is an external verification
or documentation-preview step when the tool is available.

## Consequences

The generated Code architecture is no longer directly rendered by GitHub or
Obsidian's built-in Mermaid support, but it is a better fit for an exhaustive
repository code graph.

The scoped `c4-code` query remains convenient for small Mermaid previews and
copy/paste workflows. The generated architecture note prioritizes fidelity and
renderer robustness over Markdown-native rendering.

No `.criv/state.json` schema change is required. The generated DOT note remains
ordinary vault content, and `watch --once` continues to reload the vault after
generation so validation and state serialization include the generated note in
the same run.

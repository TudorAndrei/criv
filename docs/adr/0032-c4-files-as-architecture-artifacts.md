---
id: ADR-0032
kind: decision
title: .c4 Files As Architecture Artifacts
status: accepted
date: 2026-06-19
governs:
  - src/c4.rs
  - src/check.rs
  - src/state.rs
  - src/vault.rs
---

# .c4 Files As Architecture Artifacts

## Context

[[0031-text-first-c4-architecture-formats|ADR-0031]] requires criv C4
architecture artifacts to be text/code first. Existing authored C4 diagrams live
as Mermaid blocks in Markdown notes, while generated Code architecture currently
uses DOT inside a Markdown wrapper.

The editor viewer can live in Obsidian, and later in VS Code, but those viewers
should consume architecture source rather than define it. The source file should
be easy for an LLM to generate, easy for a human to review, and easy for criv to
validate without launching a browser, Graphviz, Mermaid, Merman, or an editor.

## Decision

Use `.c4` as the criv-owned file extension for standalone C4 architecture
artifacts.

`.c4` is a filetype convention, not a new DSL. A `.c4` file contains a supported
text diagram format directly. The initial supported contents are Mermaid C4 for
System Context, Container, and Component diagrams, and DOT for generated Code
architecture.

The file format is inferred from the first meaningful non-comment line:

- `C4Context`, `C4Container`, and `C4Component` mean Mermaid C4.
- `digraph`, `graph`, `strict digraph`, and `strict graph` mean DOT.

An optional `criv:format` directive may be accepted as an assertion for clarity
or future tooling, but normal `.c4` files do not require it. If the directive is
present, it must agree with the inferred content format.

The C4 level is derived from the filename, not from an extra required directive.
Accepted level tokens are `context`, `container`, `component`, and `code`, either
as the full stem or in a numbered repository filename such as `01-context.c4`,
`02-container.c4`, `03-components.c4`, or `04-code.c4`. For Mermaid files, criv
must verify that the Mermaid C4 header agrees with the filename-derived level.
DOT has no native C4 level, so DOT `.c4` files rely on the filename token.

Standalone `.c4` artifacts must appear in `.criv/state.json` as graph nodes and
must participate in C4 validation. Elements with `criv:source` anchors should be
checked against the current source graph so diagrams stay connected to the
implementation.

Generated Code architecture should migrate from `docs/architecture/04-code.md`
to `docs/architecture/04-code.c4`. Existing Markdown output remains supported
for compatibility in other vaults, but this repository should use `.c4` for the
generated Code architecture source.

## Consequences

Humans and LLMs get a small, reviewable architecture artifact that can be opened
as source, rendered in Obsidian, or processed by future VS Code tooling.

criv keeps ownership of generation, validation, freshness checks, and state
serialization. Editor renderers such as Merman or Mermaid.js are projections
over `.c4` source, not validators.

The extension alone does not identify the inner format. criv must inspect file
contents, report clear diagnostics for unknown formats, and reject mismatches
between optional directives, inferred format, filename-derived level, and
Mermaid headers.

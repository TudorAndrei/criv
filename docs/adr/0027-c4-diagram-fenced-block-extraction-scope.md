---
id: ADR-0027
kind: decision
title: C4 Diagram Fenced Block Extraction Scope
status: accepted
date: 2026-06-18
supersedes:
  - ADR-0026
governs:
  - src/util.rs
  - src/c4.rs
  - src/vault.rs
  - src/check.rs
  - src/state.rs
  - src/query.rs
  - src/lib.rs
---

# C4 Diagram Fenced Block Extraction Scope

## Context

[[0026-mermaid-c4-diagrams-as-vault-content|ADR-0026]] recorded Mermaid C4
diagrams as validated vault content and governed the parser, vault, check,
state, query, and CLI help modules. The implementation also added
`markdown_fenced_blocks` in [[src/util.rs]] so C4 parsing can reuse the same
Markdown parsing conventions as wiki-links and headings.

Because accepted ADRs are append-only under
[[0012-adr-immutability-enforcement|ADR-0012]], the missing governed utility
scope should be corrected with a follow-up ADR rather than editing ADR-0026.

## Decision

Supersede ADR-0026 only to clarify the governed implementation scope.

Mermaid C4 diagram support includes the Markdown fenced-block extraction helper
in [[src/util.rs]], the C4 subset parser in [[src/c4.rs]], note attachment in
[[src/vault.rs]], validation in [[src/check.rs]], graph state in
[[src/state.rs]], query output in [[src/query.rs]], and CLI help registration in
[[src/lib.rs]].

The product decision remains unchanged: C4 diagrams live as Mermaid fenced
blocks in docs and ADRs; `%% criv:source` is the single source-anchor annotation;
criv validates aliases, relationships, and source target drift; criv does not
render Mermaid; and `c4-code` generates Mermaid `classDiagram` output from the
source graph.

## Consequences

Future changes to fenced Markdown block extraction that affect Mermaid C4
diagram parsing are governed by the C4 diagram decision.

The ADR history remains append-only while keeping the governance graph accurate
for every code module touched by the feature.

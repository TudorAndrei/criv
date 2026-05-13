---
id: ADR-0002
kind: decision
title: Docs And ADRs Form The Governance Graph
status: accepted
date: 2026-05-10
governs:
  - src/vault.rs
  - src/check.rs
  - src/query.rs
  - src/state.rs
---

# Docs And ADRs Form The Governance Graph

## Context

The core product idea was that source code, explanatory docs, and architectural
decisions should not drift in separate systems. The original spec called for
markdown notes with YAML frontmatter, `kind: doc` and `kind: decision`, reserved
ADR placement under `docs/adr/`, and wiki-links that resolve to source, pattern,
or note targets.

## Decision

Model `docs/` as the committed vault and `docs/adr/` as the reserved decision
directory. `kind: decision` notes must use stable ADR IDs, live under the ADR
directory, declare governed source scopes with `governs:`, and can participate
in supersession chains.

The parser and validator in [[src/vault.rs]] and [[src/check.rs]] define this
contract. Queries in [[src/query.rs]] and graph serialization in [[src/state.rs]]
then expose references, citations, governance, coverage, and orphaned docs.

## Consequences

Documentation is treated as structured repository state rather than a loose set
of markdown pages. This lets criv report broken links, unresolved governed
scopes, duplicate IDs, ADR placement mistakes, and coverage gaps.

The tradeoff is that docs must keep valid frontmatter and resolvable links. That
cost is intentional because it makes drift visible.

---
id: ADR-0005
kind: decision
title: Ast Grep Policy Search And Enforcement
status: accepted
date: 2026-05-12
governs:
  - src/structural.rs
  - src/search.rs
  - src/check.rs
  - src/enforce.rs
  - src/state.rs
---

# Ast Grep Policy Search And Enforcement

## Context

The spec made patterns first-class vault entities and required ADR
`policy.patterns` to compile to enforceable structural rules. Early criv used a
lexical compatibility fallback so search and enforcement commands could exist
before `ast-grep-core` was integrated.

## Decision

Use `ast-grep-core` for direct structural search, configured pattern search, ADR
policy search, state match storage, `criv check` failures, and `criv enforce`
failures. [[src/structural.rs]] owns compilation and source scanning; callers in
[[src/search.rs]], [[src/check.rs]], [[src/enforce.rs]], and [[src/state.rs]]
consume the resulting matches.

Accepted ADRs with policy patterns are active enforcement rules over their
effective `governs:` scopes.

## Consequences

Policy decisions can be validated against actual source structure instead of
plain text. This makes ADRs executable enough to block drift while keeping rule
definitions in markdown frontmatter.

The cost is that policy patterns must compile for the relevant language. Invalid
or overly broad patterns should be caught during check and enforcement runs.

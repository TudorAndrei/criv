---
id: ADR-0038
kind: decision
title: ADR Policy Rule Generation
status: accepted
date: 2026-06-26
governs:
  - src/lib.rs
  - src/vault.rs
  - src/config.rs
  - src/check.rs
  - src/enforce.rs
  - src/structural.rs
  - src/search.rs
  - src/state.rs
  - src/init/templates.rs
  - criv.toml
  - hk.pkl
  - assets/skills/**
  - .agents/skills/**
---

# ADR Policy Rule Generation

## Context

[[0005-ast-grep-policy-search-and-enforcement|ADR-0005]] made accepted ADR
policy patterns active enforcement rules over governed source scopes. The first
implementation required rule bodies to live in `criv.toml`, while ADR
frontmatter only named the policy pattern IDs.

That split makes enforcement work, but it leaves the decision and its structural
rule separated. It also gives hooks no direct way to tell whether generated
policy definitions are missing or stale after an ADR changes.

## Decision

Allow ADR `policy.patterns` entries to carry explicit ast-grep definitions,
including `id`, `language`, either `pattern` or `rule`, and optional `message`.
The CLI will parse and validate those inline policy definitions, generate the
corresponding `[patterns."ADR-ID/pattern-id"]` entries, and provide a check mode
that fails when generated pattern definitions drift from their ADR source.

The Rust CLI stays deterministic. It does not infer ast-grep rules from prose.
Agent skills may help authors draft and test rule definitions from a decision,
but the committed ADR frontmatter remains the source of truth.

Pre-commit and generated repository hooks must run the policy generation
freshness check before normal `criv check` and `criv enforce` execution.

## Consequences

ADRs can own both the rationale and the structural enforcement contract for a
decision. Reviewers can inspect generated ast-grep rules in normal text diffs,
and hooks can reject stale generated policy artifacts before source violations
are evaluated.

The tradeoff is that ADR authors must write precise ast-grep rules. That cost is
intentional because ambiguous natural language should not silently become
blocking enforcement.

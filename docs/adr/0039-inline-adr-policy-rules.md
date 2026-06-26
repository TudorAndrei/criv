---
id: ADR-0039
kind: decision
title: Inline ADR Policy Rules
status: accepted
date: 2026-06-26
supersedes:
  - ADR-0038
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

# Inline ADR Policy Rules

## Context

[[0038-adr-policy-rule-generation|ADR-0038]] chose generated
`[patterns."ADR-ID/pattern-id"]` entries as the bridge from ADR policy
frontmatter to ast-grep enforcement. That made stale generated artifacts a new
thing to manage.

If an ADR already carries an explicit ast-grep `pattern` or `rule`, criv can
parse and evaluate that policy directly during normal checks and enforcement.

## Decision

Parse ADR `policy.patterns` entries as executable policy definitions when they
include `language` plus either `pattern` or `rule`. `criv check`, `criv search
--rule`, `criv enforce`, and state generation evaluate those inline definitions
directly over the ADR's effective `governs:` scope.

Keep legacy ID-only policy entries working by resolving them through configured
`criv.toml` `[patterns.*]` definitions, then falling back to the ID as the raw
ast-grep pattern as before.

Do not generate policy definitions into `criv.toml`. The existing pre-commit
hook coverage is sufficient because it already runs `criv check` and `criv
enforce --stage commit`.

## Consequences

The ADR is the single source of truth for policy rationale and structural rule
body. There is no generated policy block to refresh or review.

The tradeoff is that inline ADR policy definitions must compile on every check
or enforcement run. That cost is small and keeps drift detection immediate.

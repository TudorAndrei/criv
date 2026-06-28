---
id: ADR-0041
kind: decision
title: ADR-Owned Policy Patterns
status: accepted
date: 2026-06-28
supersedes:
  - ADR-0040
governs:
  - src/config.rs
  - src/vault.rs
  - src/check.rs
  - src/enforce.rs
  - src/structural.rs
  - src/search.rs
  - src/state.rs
  - README.md
  - assets/skills/**
  - .agents/skills/**
---

# ADR-Owned Policy Patterns

## Context

[[0040-inline-only-adr-policy-rules|ADR-0040]] made ADR policy enforcement
inline-only: `criv check`, `criv search --rule`, `criv enforce`, and state
generation should use the rule definition stored in the ADR.

It still allowed standalone configured `[patterns.*]` entries for explicit
`criv search --pattern-id` usage and pattern wikilinks. That leaves an ambiguous
case: a configured pattern can be named like `ADR-0001/no-println`, even though
that identifier looks like it is owned by `ADR-0001`.

That ambiguity weakens the inline-only policy model. A reviewer or agent seeing
`ADR-0001/no-println` should not need to inspect `criv.toml` to know where the
policy rule lives.

## Decision

ADR-prefixed pattern IDs are reserved for ADR-owned inline policy patterns.

Any pattern ID shaped like `ADR-NNNN/local-id` must be declared in the matching
ADR's `policy.patterns` frontmatter. `criv.toml` must not define configured
patterns with ADR-prefixed IDs.

Standalone configured `[patterns.*]` entries may continue to exist only for
non-ADR namespaces, such as `tool/no-println` or `ui/no-alert`, where the ID does
not claim ownership by an ADR.

The CLI should not register configured patterns with ADR-prefixed IDs. Authors
must move the ast-grep body into the owning ADR before using that identifier.

## Consequences

The ADR remains the single source of truth for decision-owned policy rules.
`criv.toml` no longer has a path to shadow, imitate, or partially override an
ADR policy identifier.

Existing repositories with `[patterns."ADR-NNNN/local-id"]` entries must migrate
those definitions into the matching ADR's `policy.patterns` frontmatter or rename
them into a non-ADR namespace.

This keeps standalone configured patterns available for explicit structural
search use cases, but prevents them from looking like ADR policy rules.

---
id: ADR-0056
kind: decision
title: ADR Policy Patterns Are The Only Registered Patterns
status: accepted
date: 2026-08-02
supersedes:
  - ADR-0041
governs:
  - src/config.rs
  - src/vault.rs
  - src/check.rs
  - src/structural.rs
  - src/search.rs
  - src/state.rs
  - src/lib.rs
  - README.md
  - assets/skills/**
  - .agents/skills/**
---

# ADR Policy Patterns Are The Only Registered Patterns

## Context

[[0041-adr-owned-policy-patterns|ADR-0041]] reserved ADR-shaped IDs for inline
ADR policies but deliberately kept non-ADR `[patterns.*]` definitions in
`criv.toml`. That left two persistent named-pattern lifecycles: configured
patterns were available to `src/search.rs#fn:search_pattern_id` and could be
registered by `src/vault.rs`, while ADR policies controlled enforcement and
governance.

That split makes state ownership, pattern wiki-links, and agent discovery less
predictable. A named pattern should have one durable owner and one declared
scope.

## Decision

Inline ADR `policy.patterns` entries are the only persistent named patterns.
Each registered ID has the full `ADR-NNNN/local-id` form and resolves to a
policy definition in the owning ADR.

`criv.toml` no longer accepts non-empty `[patterns.*]` tables. The diagnostic
must explain the two migration choices: put a persistent rule in an ADR with a
stable local ID, or use positional structural search with `--lang` for an ad
hoc query.

Pattern wiki-links and the `registered-patterns` and `patterns` fields in
`criv.state.v0` remain supported, but resolve only to inline ADR policies.

`criv search --pattern-id ADR-NNNN/local-id` remains an agent-facing way to
explore exactly one inline policy. Without `--paths`, it searches the owning
ADR's effective `governs:` scope; explicit paths override that default.
`criv search --rule ADR-NNNN` continues to search every inline policy in the
ADR. Positional structural search remains the unnamed exploratory path.

This decision does not change accepted-only state registration; that is tracked
separately by issue #36.

## Consequences

`src/config.rs`, `src/vault.rs`, `src/check.rs`, `src/structural.rs`,
`src/search.rs`, `src/state.rs`, and `src/lib.rs` share one ownership model.
Documentation and the runtime skills under `README.md`, `assets/skills/`, and
`.agents/skills/` must teach ADR ownership and the migration path.

Existing repositories must migrate `[patterns.*]` definitions before updating:
use an ADR policy for a persistent rule, or retain the expression only in the
command that performs an ad hoc structural search.

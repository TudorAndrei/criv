---
id: ADR-0057
kind: decision
title: Accepted Only ADR Policy State
status: accepted
date: 2026-08-02
supersedes:
  - ADR-0056
governs:
  - src/vault.rs
  - src/state.rs
  - crates/criv-wasm/src/lib.rs
  - .obsidian/plugins/criv/test/core.test.mjs
  - extensions/vscode-criv/test/unit/stateModel.test.ts
---

# Accepted Only ADR Policy State

## Context

[[0056-adr-policy-patterns-are-the-only-registered-patterns|ADR-0056]] makes
inline ADR policy patterns the sole persistent named patterns and deliberately
leaves their lifecycle in generated state for a follow-up decision. Draft and
otherwise non-accepted decisions must remain visible in the vault graph, while
their proposed policies must not be published as active generated-state
registrations.

## Decision

`criv.state.v0` registers and scans an inline policy pattern only when its
owner is a decision note with the exact status `accepted`. The predicate is
case-sensitive and applies at the `src/vault.rs#fn:registered_policy_patterns`
state registration boundary, not during general policy resolution.

Promoting a policy owner from a non-accepted status to `accepted` has no prior
state entry, so incremental generation scans every source file in the ADR's
effective `governs:` scope. Demoting an accepted owner to any other status
removes its ID from both `registered-patterns` and `patterns`, including all
previously cached matches. Unchanged accepted patterns retain the existing
incremental reuse behavior.

The schema remains `criv.state.v0`; non-accepted ADRs remain graph notes and
editor consumers must continue to parse the same field shapes. The shared
fixture therefore includes a draft decision but exposes only its accepted
counterpart in state policy fields.

This decision changes generated-state registration only.
`src/check.rs#fn:policy_violations`, `src/enforce.rs#fn:policy_violations`,
`criv search --pattern-id`, `criv search --rule`, and policy wiki-link
resolution retain their existing status-aware or status-agnostic behavior.

## Consequences

Generated state represents active, settled policy only, while proposals remain
inspectable through normal vault, search, and graph behavior. Consumers cannot
mistake a draft policy for an enforced registration, and a policy promotion or
demotion updates state safely without a schema migration.

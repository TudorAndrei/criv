---
id: ADR-0114
kind: decision
title: Reconcile file discovery source scopes
status: accepted
date: 2026-08-16
supersedes:
  - ADR-0082
  - ADR-0092
  - ADR-0111
governs:
  - Cargo.toml
  - src/check.rs
  - src/config.rs
  - src/discovery/**/*.rs
  - src/refresh.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/paths.rs
  - src/vault.rs
  - src/watch.rs
  - scripts/performance/src/**/*.rs
---

# Reconcile File Discovery Source Scopes

## Context

[[0112-direct-ignore-file-discovery|ADR-0112]] replaces
`src/source/index.rs` with `src/discovery/` and `src/source/catalog.rs`. Three
effective decisions still govern the deleted path. The source change is a
replacement, not a one-to-one Git rename, so automatic source reconciliation
cannot change the accepted decisions.

## Decision

Make ADR-0112 the current file-discovery and live Source implementation
decision. Keep the three profile rules, path identity, link and error rules,
stable ordering, target lookup, one-shot and live parity, watch generation,
recovery, and verification rules from ADR-0111 and ADR-0092, with the narrow
`ignore` control-file exception in ADR-0112.

Keep the removal of the standalone search command from ADR-0082. Keep the
internal exact, suffix, and basename Source target lookup. Do not restore fuzzy
ranking, frecency, or a general search command.

Move the active source scope to the new discovery and Source catalog modules.
This decision changes source ownership only. It does not change the behavior
that the three superseded decisions define.

## Consequences

Effective ADR scopes resolve to current source files. The old index path stays
only in historical decisions. Current policy and architecture references use
the direct discovery module and the Source catalog.

## Alternatives Considered

### Keep an empty compatibility file

Rejected. A source tombstone would make governance resolve to a file that has
no current responsibility.

### Edit the accepted ADRs

Rejected. A deleted source path is not a one-to-one Git rename. The accepted
decisions are immutable outside the proven rename workflow.

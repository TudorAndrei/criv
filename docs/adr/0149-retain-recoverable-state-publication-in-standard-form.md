---
id: ADR-0149
kind: decision
title: Retain Recoverable State Publication in Standard Form
status: accepted
date: 2026-10-30
supersedes:
  - ADR-0094
governs:
  - src/config.rs
  - src/lib.rs
  - src/state.rs
  - src/state/snapshots.rs
  - src/refresh.rs
  - src/query.rs
---

# Retain Recoverable State Publication in Standard Form

## Context

[[0094-automatic-recoverable-state-publication|ADR-0094]] contains an accidental
second H1 in its issue-reference prose. Its accepted bytes are immutable.
[[0148-separate-markdown-formatting-from-vault-validation|ADR-0148]] requires
one title heading and standard sections in every active ADR.

## Decision

Retain the complete recoverable State publication contract of ADR-0094.
Its transaction sequence, preflight, lock, quarantine, rollback, recovery,
post-commit cleanup warnings, tests, and confinement requirements remain
normative by reference. This successor changes document structure, not product
behavior, storage schemas, or ownership.

State publication owns automatic retention with a positive `[state] keep`
bound, default 20. `.criv/state.json` is the authoritative commit record.
Readers recover before returning local snapshots. Before commit, failure rolls
back to the prior authoritative revision; after commit, publication succeeds
and cleanup failures warn. Live memory follows that same commit boundary.

Keep the absence of the public `state` command group and keep `query diff`
resolution for local snapshots and Git refs. Preserve all decisions that
ADR-0094 itself superseded through the existing supersession chain.

## Consequences

The active decision meets the standard without editing historical ADR-0094.
All publication behavior and its existing verification obligations remain.

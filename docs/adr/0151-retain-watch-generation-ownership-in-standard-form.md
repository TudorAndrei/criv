---
id: ADR-0151
kind: decision
title: Retain Watch Generation Ownership in Standard Form
status: accepted
date: 2026-10-30
supersedes:
  - ADR-0141
governs:
  - src/watch.rs
  - src/refresh.rs
---

# Retain Watch Generation Ownership in Standard Form

## Context

[[0141-own-ordinary-refresh-checks-in-the-watch-generation|ADR-0141]] records
the correct ownership but lacks the sections required by
[[0148-separate-markdown-formatting-from-vault-validation|ADR-0148]].

## Decision

ActiveWatchGeneration owns the complete ordinary refresh operation. It takes
the repository handle and refresh cause and returns an owned summary, failure,
or reconfiguration request.

Keep both configuration comparisons. The precommit callback rejects a changed
or unreadable configuration before publication. Compare again after the
attempt, with reconfiguration taking priority over the refresh result. The
session retains event polling, suspension, recovery watches, retry timing,
failure reports, and generation replacement.

Retain ADR-0092 candidate acceptance: one configuration snapshot and an initial
RefreshSession refresh, without the ordinary-refresh guard. Candidate events
remain queued. Source reuse and failed-refresh retry remain in RefreshSession
under ADR-0126. Publication transaction, rollback, callback position, and lock
order remain unchanged. Rejected publication preserves the last successful
State and refresh result.

Tests may use a session-local checkpoint at the existing precommit callback,
not a production callback interface or process-global test state.

## Consequences

This successor supplies the standard structure without rewriting accepted
history. Candidate acceptance, one-shot refresh, State schemas, publication
order, retry intervals, and reports keep their existing contracts.

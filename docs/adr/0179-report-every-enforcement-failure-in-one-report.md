---
id: ADR-0179
kind: decision
title: Report Every Enforcement Failure in One Report
status: accepted
date: 2026-11-06
governs:
  - src/enforce.rs
  - src/check.rs
  - src/adr.rs
  - src/adr/gate.rs
  - src/adr/receipt.rs
  - src/adr/reconcile_transaction.rs
  - src/adr/identity_reconcile.rs
  - src/adr/source_reconcile.rs
  - src/git.rs
---

# Report Every Enforcement Failure in One Report

## Context

[[0163-separate-the-adr-gate-from-one-reconcile-transaction|ADR-0163]] has CI
call `reconcile_identities` in check mode before enforcement. That command
prints its target and mapping to standard output, so
`criv enforce --stage ci --format json` with `CRIV_BASE_REF` wrote text before
the JSON object. A conflict ended the run with an error that had no code.

Enforcement reported only the first failing category. A run with a policy
violation and an ADR edit showed the policy violation and hid the edit.

`criv check --changed` promotes a staged deletion, rename, decision change, or
`criv.toml` change to a full check
([[0067-staged-changes-are-a-partial-check-scope|ADR-0067]]). Commit and push
enforcement always scanned only the changed files. A commit that added a
decision with a new policy did not scan the unchanged files that the policy
governs.

The ADR gate dropped a malformed receipt without a word, and it skipped the
decision scope check when the previous `criv.toml` did not parse. Both
receipts had their own proof entry point, and the gate computed the composed
renames and the commit proofs again for each changed entry.

## Decision

- `adr::identity_conflict` answers the CI identity check without output. CI
  enforcement puts a conflict in its report as an `adr-id-conflict` violation
  that names each mapping. Only `criv adr reconcile` prints.
- Enforcement reports every failing category. The JSON report holds every
  violation, every failing code in `codes` in priority order, and the first
  code as `code` with its `fix`. The exit code follows the first category:
  ADR ID conflict, policy, import policy, ADR immutability, then validation
  errors.
- `check::changed_scope` is the one rule for check and enforcement. A change
  that `criv check --changed` promotes to a full check also makes commit and
  push enforcement scan every governed file.
- The `Receipt` trait carries `allows_transaction` and `target_is_stable`. The
  gate reads each receipt once and asks the same questions of both. It
  composes the renames once and proves each compared commit once. A receipt
  that does not parse is a violation.
- The gate skips the scope check only when the previous `criv.toml` is
  absent. When it cannot be read or parsed and the change touches a decision,
  that is a violation.
- A reconcile `Context` holds the merge-base-to-`HEAD` change set, computed
  once, and the resolved `HEAD` commit instead of the name `HEAD`.
- Receipts hash with `content_hash::of_bytes`, which is the one hash ADR-0163
  asks for; the receipt module keeps no wrapper.
- A staged comparison on an unborn `HEAD` has no old commit. A `HEAD` that
  cannot be read for any other reason is an error.

This refines ADR-0163 and
[[0140-own-the-complete-adr-change-decision|ADR-0140]]. The receipt schemas
and the gate's per-entry rules do not change.

## Consequences

`criv enforce --stage ci --format json` always writes one JSON object.

A commit that adds a policy fails on existing code that breaks it, as CI
would.

A stale receipt with a schema this version cannot read blocks enforcement
until it is removed.

---
id: ADR-0182
kind: decision
title: Record Prior Snapshot Hashes in the Publication Transaction
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0167
governs:
  - src/config.rs
  - src/lib.rs
  - src/state.rs
  - src/state/publication.rs
  - src/state/snapshots.rs
  - src/repository.rs
  - src/watch.rs
  - src/query.rs
  - crates/criv-state-wire/src/layout.rs
---

# Record Prior Snapshot Hashes in the Publication Transaction

## Context

[[0167-publish-state-without-a-staging-copy|ADR-0167]] keeps the recoverable
State publication of
[[0094-automatic-recoverable-state-publication|ADR-0094]]. Its transaction
record held the full contents of every snapshot in the store, so each
publication wrote all retained snapshots a second time. Rollback rewrote each
one from the record.

A snapshot file is named by the content hash of its contents, and retention
moves removed snapshots to the quarantine. A rollback needs only the set of
hashes that were stored before the publication: it removes each stored hash
outside that set and moves each quarantined snapshot back. The record also
kept the retention count, which no step read.

The publication module listed the snapshot directory with its own name filter,
next to the reconciliation of the snapshot module. The configuration and the
snapshot module each checked that the retention count is positive.

## Decision

Retain the complete publication contract of ADR-0167, with these changes.

The transaction record holds `prior_hashes`, the hashes stored before the
publication, in place of the snapshot contents and the retention count. Its
schema is `criv.state-publication.v2`. Rollback removes each stored snapshot
whose hash is not in `prior_hashes` and moves the quarantined removals back.
A candidate whose hash was already stored stays, because its contents are the
same.

Recovery rejects a record of an earlier schema, including a `v1` record and
its `staged` phase, as an unsupported schema. A `v1` record exists only when a
publication was interrupted; `.criv/` is generated local state, so the
operator removes it and refreshes.

`snapshots::stored_hashes` is the one listing of stored snapshot names.
Reconciliation and rollback both use it, and the publication plan returns the
prior hash set from the same reconciled view.

The configuration parses `state.keep` into a `NonZeroUsize`. The snapshot
store takes that type and does not check the count again.

## Consequences

A publication writes the transaction record in a size that does not grow with
the retained snapshots.

An upgrade across an interrupted `v1` publication fails recovery until the
operator removes `.criv/`.

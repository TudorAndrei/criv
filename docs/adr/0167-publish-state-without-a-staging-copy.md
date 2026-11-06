---
id: ADR-0167
kind: decision
title: Publish State Without a Staging Copy
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0149
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

# Publish State Without a Staging Copy

## Context

[[0149-retain-recoverable-state-publication-in-standard-form|ADR-0149]]
retains the recoverable State publication of
[[0094-automatic-recoverable-state-publication|ADR-0094]]. Its step 4 stages
the candidate snapshot, index, and `latest` below `.criv/state-stage/`. No
step reads these files: the transaction record already holds the candidate
State and index, and installation and recovery write from the record.

The snapshot module also kept a second, test-only store with publish, list,
and prune functions of the removed `criv state` commands. Its retention rule
protected the old `latest`, not the candidate. A local snapshot name was any
hexadecimal text, so a short Git commit name such as `abc123` took the lock
and searched the local store first.

The publication lock waited 2 seconds and retried every 25 ms. The watch lock
of [[0095-operating-system-watch-session-lock|ADR-0095]] failed at once. The
two locks used two different loops.

## Decision

Retain the complete publication contract of ADR-0149 and ADR-0094, except
step 4. A transaction writes its record, moves retention targets to the
quarantine, installs the candidate snapshot, index, and `latest` from the
record, and then commits `.criv/state.json`. The `.criv/state-stage/`
directory and the `staged` transaction phase are gone. A record that an
earlier release left in the `staged` phase still loads.

The production publication path is the only snapshot store. Retention tests
publish through it. A local snapshot name is `latest` or a complete content
hash; any other value is a Git ref for `query diff`.

An invalid snapshot index is repairable, as ADR-0094 decides: it orders
nothing, and the next publication writes a new index. A snapshot outside the
index is ordered by its modification time and then its hash. On a platform without
modification times, the hash alone orders it.

All criv operating-system locks use one rule, `repository::lock_within`: the
lock retries every 25 ms until the wait of its role has passed. The State
publication lock waits 2 seconds, and its timeout message reads that value.
The watch-session lock waits zero, so it fails at once as ADR-0095 decides.

## Consequences

A publication writes three fewer files. The transaction phases are
`prepared`, `quarantined`, `installed`, `committed`, and `cleanup`.

`query diff abc123 latest` reads Git directly for a short commit name. A
complete hash that is not stored locally still falls back to Git.

A new lock role chooses only its wait.

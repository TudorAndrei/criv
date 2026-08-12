---
id: ADR-0093
kind: decision
title: Recoverable State Publication Transactions
status: accepted
date: 2026-08-11
supersedes:
  - ADR-0068
governs:
  - src/state.rs
  - src/snapshots.rs
  - src/refresh.rs
  - src/query.rs
---

# Recoverable State Publication Transactions

## Context

[[0007-content-addressed-state-and-diffing|ADR-0007]] defines current State,
content-addressed snapshots, and `latest` as one publication. The snapshot
lifecycle in [[0068-bounded-local-snapshot-lifecycle|ADR-0068]] adds a durable
order index and bounded retention. It permits recoverable multi-file
publication, but it does not define which revision is authoritative after each
failure.

The present sequence does not give one successful-publication result.
`src/state.rs#fn:write_state` replaces `.criv/state.json` before
`src/snapshots.rs#fn:publish` writes the candidate snapshot and `latest`,
reconciles the store, removes retention targets, and writes the order index. A
corrupt managed snapshot or a later file-system error can stop that work after
some files change. `src/refresh.rs#type:RefreshSession` changes its in-memory
State only after the complete refresh returns. The disk State can therefore
contain the candidate while `latest`, retained snapshots, the index, and live
memory contain different revisions.

Per-file atomic replacement cannot make these files one atomic file-system
operation. Recovery needs a commit record, an exclusive writer, a durable
transaction record, and a reversible retention step. The exact live watch
generation contract remains in
[[0092-transactional-live-watch-generations|ADR-0092]]. The source graph cache
can contain a newer observed file-system view as permitted by
[[0073-effective-adr-governance-and-source-reconciliation|ADR-0073]]. Generated
architecture is also outside this State publication transaction.

## Decision

Define `.criv/state.json` as the authoritative State publication commit record.
Before its atomic replacement, the prior State revision is authoritative.
After its atomic replacement, the candidate revision is authoritative. If no
prior State exists, the authoritative pre-commit condition is no published
State.

Create one deep `StatePublication` module with one publication interface. It
owns the disk transaction for `.criv/state.json`, the candidate snapshot,
`.criv/latest`, the snapshot order index, and retention changes. State building
and serialization stay with `src/state.rs`. The refresh owner changes its live
in-memory State only after the interface reports that the State commit is
complete. ADR-0007 remains active: State retains product ownership of the
serialized State, its hash, and the latest-State semantics. `StatePublication`
owns the disk transaction that implements those semantics.

Use one operating-system publication lock for each repository. A writer holds
it from recovery and preflight through transaction completion. Another writer
waits for a short fixed interval and then fails without changes if it cannot
get the lock. A process exit releases the lock. The wait interval is one stable
implementation constant and its timeout has a stable diagnostic.

After it gets the lock, a writer first recovers an incomplete transaction.
It then completes a mutation-free preflight. Preflight validates the candidate
State, content hash, retention value, confined paths, regular-file types, all
managed snapshots, `latest`, and the order index. It calculates the complete
publication, reconciliation, and retention plan. A missing or invalid index is
repairable and becomes part of that plan. A corrupt managed snapshot stops the
operation before any State publication artifact changes. It is never deleted
automatically.

Use a confined durable transaction record and a confined transaction
quarantine below `.criv/`. The record identifies the prior State condition,
the candidate hash, the prior and candidate pointers and index, newly installed
files, quarantined files, and the current phase. Synchronize the record and its
parent directory before the first publication change. A transaction must use
this order while it holds the lock:

1. Recover an earlier incomplete transaction.
2. Complete preflight.
3. Write and synchronize the transaction record.
4. Stage the candidate snapshot, index, and `latest`.
5. Move retention targets to the transaction quarantine.
6. Install the candidate snapshot.
7. Install the candidate index.
8. Install candidate `latest`.
9. Atomically replace and synchronize `.criv/state.json` as the commit.
10. Change the live in-memory State.
11. Delete quarantined files and the transaction record.

Each installed file uses a synchronized temporary file, atomic replacement,
and parent-directory synchronization. Quarantine moves stay on the same file
system. They replace destructive pre-commit deletion and keep rollback
possible. Until the State commit, CLI readers cannot observe the staged
publication because the writer holds the publication lock.

A failure before the State commit starts rollback while the lock is held.
Restore the prior `latest`, prior index, all quarantined snapshots, and the
prior managed snapshot set. Keep the prior disk State and live in-memory State.
For a first publication, rollback restores the no-State condition and removes
the candidate publication artifacts.

If rollback also fails, keep the transaction record and report that recovery
is required. A later recovery compares the hash in `.criv/state.json` with the
transaction record. The prior hash, or an absent State for a first
publication, requires rollback. The candidate hash requires completion of the
candidate publication. Any other valid State, an invalid State, path
ambiguity, or a recovery failure stops the reader or writer without returning
possibly inconsistent snapshot data.

Every CLI operation that reads or changes local snapshot publication data must
use the `StatePublication` interface. This includes State list and prune,
snapshot lookup for `src/query.rs#fn:load_snapshot`, and new publication. It
gets the lock and recovers an incomplete transaction before it reads the
store. With no transaction to recover, State list stays mutation-free. Direct
editor consumers can read `.criv/state.json` without the lock because that
file alone is the commit record.

After the State commit, the candidate publication is successful. The live
session must use the candidate State, and the command must not report
publication failure. Failure to delete quarantined files or the transaction
record produces a warning. It keeps the transaction record so a later reader
or writer can complete cleanup. The managed snapshot set already satisfies the
retention limit because retention targets left that set before commit.

Add a controlled file-system seam to the module. Deterministic tests cause a
failure at each preflight, stage, synchronization, replacement, quarantine,
rollback, commit, and cleanup step. Each test starts recovery as a new process
would and proves these rules:

- A pre-commit failure returns every authoritative artifact to the prior
  revision.
- A post-commit interruption recovers every artifact to the candidate
  revision.
- Live in-memory State follows the commit result.
- No valid retained snapshot is lost, and no corrupt snapshot is deleted.
- A reader fails closed when recovery cannot complete.

Also test first publication, repeated identical State, the retention limit,
two concurrent writers, a lock timeout, rollback failure, and cleanup failure.
Portable controlled tests run on every supported platform. Real file-system
tests run on Linux, macOS, and Windows and cover the platform lock, replacement,
directory synchronization, and quarantine behavior.

Retain the snapshot names, deterministic publication order, positive `keep`
value, latest protection, local lookup, Git-ref fallback, list output, prune
output, and dry-run behavior from ADR-0068. Prune uses the publication lock and
the same reversible mutation rules. Dry-run gets the lock and performs
preflight and recovery, but it does not start a new pruning transaction.

## Consequences

A command cannot report a normal pre-commit failure after it has made a new
State revision authoritative. Snapshot readers see a recovered store or a
clear failure, and a live watcher changes memory at the same commit point as
disk State.

Publication does more work. It needs a cross-platform lock, a durable
transaction record, quarantine storage, directory synchronization, and
controlled file-system tests. Retention files can remain in quarantine after a
successful commit until later cleanup, but they are not part of the managed
retained set.

The contract is recoverable, not one atomic operation across all files. A
process or machine stop can leave a transaction record, but the State commit
record gives recovery one deterministic direction. Source graph cache and
generated architecture consistency remain separate refresh concerns.

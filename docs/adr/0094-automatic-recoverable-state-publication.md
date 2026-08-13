---
id: ADR-0094
kind: decision
title: Automatic Recoverable State Publication
status: accepted
date: 2026-08-11
supersedes:
  - ADR-0093
governs:
  - src/config.rs
  - src/lib.rs
  - src/state.rs
  - src/state/snapshots.rs
  - src/refresh.rs
  - src/query.rs
---

# Automatic Recoverable State Publication

## Context

[[0093-recoverable-state-publication-transactions|ADR-0093]] defines one
recoverable transaction for current State, content-addressed snapshots,
`latest`, the order index, and retention. It also retains the public `criv
state list` and `criv state prune` commands from
[[0068-bounded-local-snapshot-lifecycle|ADR-0068]].

Snapshot retention is internal State publication maintenance. A user must not
need to inspect the store, select one-off retention values, preview deletion,
or start pruning. The public command group exposes those internal operations
and makes the configured retention rule optional for a manual run. GitHub issue

# 115 requires automatic retention while it keeps local snapshot resolution for

`query diff`.

The command correction does not remove the failure-order contract from
ADR-0093. The current implementation still replaces `.criv/state.json` before
snapshot publication, reconciliation, and pruning complete. A corrupt managed
snapshot or later file-system error can therefore leave disk State, `latest`,
the retained set, the index, and live in-memory State at different revisions.
This successor must restate the complete recoverable publication behavior as
well as remove the public commands.

## Decision

Keep `[state] keep` as a positive configured value with default `20`. It is the
maximum number of unique valid local snapshots in the managed retained set
after each successful State publication. Publishing identical content uses one
retention slot and makes its hash the newest publication. A valid `latest`
snapshot is always protected.

Remove the complete public `criv state` command group, including list, prune,
keep override, dry-run, text output, and JSON output. Do not add a deprecation
alias or a replacement maintenance command. Remove the group from CLI help and
the exported usage specification. These command forms fail as unknown commands.

Keep `criv query diff <a> <b>`. It resolves `latest` and retained local hashes
before it tries a Git ref. Local retention remains convenience history, and a
hash removed by automatic retention no longer resolves locally. Git-ref lookup
of tracked `.criv/state.json` stays independent from the local snapshot store.

Define `.criv/state.json` as the authoritative State publication commit record.
Before its atomic replacement, the prior State revision is authoritative.
After its atomic replacement, the candidate revision is authoritative. If no
prior State exists, the authoritative pre-commit condition is no published
State.

Create one deep `StatePublication` module with one publication interface. It
owns the disk transaction for `.criv/state.json`, the candidate snapshot,
`.criv/latest`, the snapshot order index, and automatic retention. State
building and serialization stay with `src/state.rs`. The refresh owner changes
its live in-memory State only after the interface reports that the State commit
is complete. [[0007-content-addressed-state-and-diffing|ADR-0007]] remains
active: State retains product ownership of the serialized State, its hash, and
latest-State semantics. `StatePublication` owns the disk transaction that
implements those semantics.

Use one short operating-system publication lock for each repository. A writer
holds it from recovery and preflight through transaction completion. Snapshot
lookup gets the same lock and completes recovery before it reads a local
snapshot. Another writer waits for one short fixed interval and then fails
without changes if it cannot get the lock. A process exit releases the lock.
The wait interval is one stable implementation constant with a stable timeout
diagnostic.

After it gets the lock, a writer first recovers an incomplete transaction. It
then completes a mutation-free preflight. Preflight validates the candidate
State, content hash, configured retention value, confined paths, regular-file
types, all managed snapshots, `latest`, and the order index. It calculates the
complete publication, reconciliation, and retention plan. A missing or invalid
index is repairable and becomes part of that plan. A corrupt managed snapshot
stops the operation before any State publication artifact changes. It is never
deleted automatically.

Use a confined durable transaction record and transaction quarantine below
`.criv/`. The record identifies the prior State condition, candidate hash,
prior and candidate pointers and index, newly installed files, quarantined
files, and current phase. Synchronize the record and parent directory before
the first publication change. A transaction uses this order while it holds the
lock:

1. Recover an earlier incomplete transaction.
2. Complete preflight.
3. Write and synchronize the transaction record.
4. Stage the candidate snapshot, index, and `latest`.
5. Move automatic retention targets to the transaction quarantine.
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
ambiguity, or recovery failure stops the reader or writer without returning
possibly inconsistent snapshot data.

After the State commit, the candidate publication is successful. The live
session must use the candidate State, and the command must not report
publication failure. Failure to delete quarantined files or the transaction
record produces a warning. Keep the transaction record so a later query or
writer can complete cleanup. The managed snapshot set already satisfies the
retention limit because retention targets left that set before commit.

Add a controlled file-system seam to `StatePublication`. Deterministic tests
cause a failure at each preflight, stage, synchronization, replacement,
quarantine, rollback, commit, and cleanup step. Each test starts recovery as a
new process would and proves these rules:

- A pre-commit failure returns every authoritative artifact to the prior
  revision.
- A post-commit interruption recovers every artifact to the candidate
  revision.
- Live in-memory State follows the commit result.
- No valid retained snapshot is lost, and no corrupt snapshot is deleted.
- A local snapshot reader fails closed when recovery cannot complete.

Also test first publication, repeated identical State, the automatic retention
limit, two concurrent writers, a lock timeout, rollback failure, and cleanup
failure. Portable controlled tests run on every supported platform. Real
file-system tests run on Linux, macOS, and Windows and cover the platform lock,
replacement, directory synchronization, and quarantine behavior.

CLI and integration tests prove that `state`, `state list`, and `state prune`
are absent from help and the usage specification and fail as unknown commands.
They also prove that successful publication applies `[state] keep`
automatically, identical State uses one slot, `latest` stays protected,
`query diff HEAD latest` works, and an automatically removed hash no longer
resolves locally.

All publication reads, writes, quarantine moves, and removals remain confined
beneath the selected vault `.criv/` directory as required by
[[0044-vault-write-confinement|ADR-0044]]. Snapshot and transaction paths must
be regular files beneath real directories. A symbolic link, junction,
path-escape ambiguity, or unsupported file type fails closed before mutation.

The source graph cache can contain a newer observed file-system view as
permitted by
[[0073-effective-adr-governance-and-source-reconciliation|ADR-0073]]. Generated
architecture remains outside this State publication transaction. The watch
generation contract remains in
[[0092-transactional-live-watch-generations|ADR-0092]].

## Consequences

Snapshot retention has one automatic policy. Users configure its bound and use
`query diff` for comparisons, but they do not operate the local store.

A command cannot report a normal pre-commit failure after it has made a new
State revision authoritative. Snapshot lookup sees a recovered store or a
clear failure, and a live watcher changes memory at the same commit point as
disk State.

Publication needs a cross-platform lock, durable transaction record,
quarantine storage, directory synchronization, and controlled file-system
tests. The contract is recoverable, not one atomic operation across all files.
A process or machine stop can leave a transaction record, but the State commit
record gives recovery one deterministic direction.

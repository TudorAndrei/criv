---
id: ADR-0068
kind: decision
title: Bounded Local Snapshot Lifecycle
status: accepted
date: 2026-08-03
governs:
  - src/config.rs
  - src/state.rs
  - src/snapshots.rs
  - src/query.rs
  - src/lib.rs
targets:
  symbols:
    - src/config.rs#type:Config
    - src/state.rs#fn:write_state
    - src/query.rs#fn:load_snapshot
    - src/lib.rs#type:Command
---

# Bounded Local Snapshot Lifecycle

## Context

[[0007-content-addressed-state-and-diffing|ADR-0007]] makes every successful
state publication write `.criv/state.json`, a content-addressed file below
`.criv/snapshots/`, and the hash in `.criv/latest`. `query diff` can resolve a
known local hash, but the CLI cannot enumerate those hashes and nothing removes
old snapshots. Because `.criv/` is ignored local state and `watch --once` is a
normal commit workflow step, distinct repository states otherwise accumulate
without a bound.

Content addressing alone does not establish recency. File modification times
can be equal, copied, or changed independently, and publishing content already
present in the store does not necessarily rewrite its snapshot file. Retention
therefore needs a durable publication order separate from snapshot file
metadata. Existing stores have no such index, so the lifecycle must also have a
deterministic bootstrap and repair rule.

Git-ref diffing is a separate capability. It reads `.criv/state.json` from a
tracked tree through embedded Git access; local snapshot pruning must neither
invoke Git nor change what a Git ref means.

GitHub issue #9 requires a bounded store and a typed inspection surface without
weakening the confinement guarantees in
[[0044-vault-write-confinement|ADR-0044]].

## Decision

Add a typed `[state] keep` setting to `criv.toml`. It is a positive integer and
defaults to `20`; zero and malformed values are configuration errors. The
initializer emits the default explicitly. The value is the maximum number of
unique, valid local snapshots retained after a successful publication. A
one-off prune command may supply another positive keep value without changing
configuration.

Make a snapshot-store module own local snapshot publication, lookup,
enumeration, reconciliation, and pruning. Keep snapshots at
`.criv/snapshots/<hash>.json`, the latest pointer at `.criv/latest`, and an
atomic publication-order index below `.criv/snapshots/`. The index stores each
hash at most once from oldest to newest. Publishing an existing hash moves that
hash to the newest position; repeated identical states therefore consume one
retention slot while still recording their latest publication order.

Every store operation reconciles the index with confined regular snapshot
files before relying on it. Missing indexed files are removed. Valid orphan
files are added deterministically by modification time and then hash, with the
hash named by `.criv/latest` moved to the newest position. A missing or corrupt
index is rebuilt by the same rule, allowing pre-index stores to bootstrap.
Equal timestamps never make output nondeterministic. Unrecognized files are
ignored. A recognized hash file whose content is not a valid matching State
snapshot is reported as corruption and is never automatically deleted.

After state and snapshot bytes are durably published, publication records the
hash, updates `.criv/latest`, writes the reconciled order atomically, and prunes
oldest entries until the configured bound is met. The hash named by a valid
`.criv/latest` is always protected, even if an inconsistent store would
otherwise place it outside the bound. Pruning updates the order index
atomically after confined removals. Reconciliation makes interrupted sequences
recoverable on the next operation; it does not claim a multi-file atomic
transaction.

Add typed top-level commands:

- `criv state list [--format text|json]` lists valid local snapshots newest
  first with stable hash, publication position, byte size, and latest status;
- `criv state prune [--keep N] [--dry-run] [--format text|json]` reports and,
  unless dry-run, removes the oldest unprotected snapshots required by the
  selected bound.

Both commands use stable machine-readable rows in JSON and deterministic text
rendering. Dry-run performs the same reconciliation and selection but makes no
filesystem changes. `state list` is read-only: when an index needs repair it
uses the reconciled view without persisting it. An empty or not-yet-generated
store produces an empty list and a no-op prune, not an error.

Local snapshot lookup for `query diff` delegates to the same store. `latest`
continues to mean the local latest pointer. A hash removed by retention is no
longer locally resolvable. Non-hash values remain Git refs, and their
`.criv/state.json` content remains available regardless of local pruning.
Pruning never walks refs, retains files on behalf of refs, or modifies tracked
content.

All reads, writes, and removals remain confined beneath the selected vault's
`.criv/` directory. Snapshot and index paths must be regular files beneath real
directories; symlink or path-escape ambiguity fails closed before mutation.

## Consequences

Fresh vaults retain at most twenty distinct local states by default, and busy
repositories no longer grow the ignored snapshot store indefinitely. Users can
inspect hashes without listing implementation directories and can preview an
explicit retention change before deleting anything.

The publication-order index adds recoverable local metadata and a small amount
of work to publication. Deterministic reconciliation preserves existing
snapshots when upgrading, but pre-index recency is necessarily inferred from
file metadata; only publications after the index exists have exact order.

Retention deliberately bounds convenience history, not source-control history.
Users who need a durable comparison point must retain a Git ref containing the
corresponding `.criv/state.json` or copy data outside the managed local store.

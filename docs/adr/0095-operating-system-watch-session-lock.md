---
id: ADR-0095
kind: decision
title: Operating-System Watch Session Lock
status: accepted
date: 2026-08-11
governs:
  - src/watch.rs
  - src/glob.rs
  - .github/workflows/ci.yml
---

# Operating-System Watch Session Lock

## Context

Both live `criv watch` and `criv watch --once` must have one refresh owner for
their complete lifetime. The current `.criv/watch.lock` protocol uses atomic
file creation, but it closes the created file after it writes a PID and an
optional process start time. The file path and its text are the only ownership
claims.

This protocol has three ownership races. A second process can see the empty
file between creation and the owner write, classify it as abandoned, remove
it, and become another owner. Two reclaimers can both classify an old file as
abandoned, and a later reclaimer can remove the file that the first reclaimer
just created. A former owner also removes the current path during drop without
checking identity, so it can remove a newer owner file.

Process inspection does not repair these races. Linux and macOS run `ps` and
fall back to `kill -0`. Windows does not inspect the process and can treat a
natural lock from a dead owner as live forever. Malformed, partial, unreadable,
and old text currently triggers path removal, which makes each race wider.
GitHub issue #102 requires one atomic cross-platform ownership mechanism, safe
process-exit recovery, and conditional release.

[[0092-transactional-live-watch-generations|ADR-0092]] assigns one
`LiveWatchSession` to each active generation, but it does not define exclusion
between processes.
[[0094-automatic-recoverable-state-publication|ADR-0094]] defines a separate
short operating-system lock for each State publication transaction. A live
watch cannot hold that publication lock for its full lifetime because snapshot
lookup and automatic recovery also need it.

## Decision

Keep two different lock roles. A watch-session lock permits one live or
one-shot refresh owner for the full session. A State publication lock protects
one disk publication or local snapshot read. A live watcher or one-shot run
gets the watch-session lock first and then gets the publication lock for each
automatic publication. Snapshot lookup for `query diff` gets only the
publication lock. This order is fixed; code must not get the watch-session lock
while it holds the publication lock.

Create one deep `WatchSessionLock` module. It owns confined file open and
creation, the operating-system file lock, diagnostic publication, contention
diagnostics, and release. `LiveWatchSession` owns one lock guard for its full
lifetime. `watch --once` owns one guard for its full refresh. Acquire the guard
before configuration load, session construction, State publication, or
generated architecture mutation.

Keep `.criv/watch.lock` as one persistent regular file below the selected
repository `.criv/` directory. Open or create it through one confined helper
that rejects a symbolic link, junction, directory, non-regular file, path
escape, or non-real parent directory as required by
[[0044-vault-write-confinement|ADR-0044]]. Never replace, rename, or delete this
file during normal acquisition, recovery, or release.

Use an exclusive non-blocking operating-system file lock on the open file.
`std::fs::File::try_lock` is the ownership operation on supported Rust. The
successfully locked open file handle is the complete owner identity. PID,
process start time, file contents, modification time, and path presence are not
ownership authority.

If another owner holds the lock, fail immediately with exit status 1 before
refresh work. Do not wait, retry, or queue another live or one-shot watch. The
stable error states that another watch session owns State refresh and tells the
user not to start another watch or `watch --once` while it is active.

After lock success, truncate, write, and synchronize this diagnostic record:

```text
schema criv.watch-lock.v1
pid <process-id>
mode live|once
```

The record is diagnostic only. On contention, include its mode and PID only
when the complete record can be read and validated from the already opened
file. A missing, old, malformed, or partial record does not change the active
lock result. If the owner cannot truncate, write, or synchronize its new
record, close the handle and fail before refresh. Use separate stable messages
for contention, unsafe path, operating-system lock failure, and diagnostic
publication failure.

Release ownership only by closing the exact locked file handle. Do not remove,
rename, truncate, or update the file during release. Normal return, error
return, panic unwind, and process termination close the handle and let the
operating system release the lock. A forced process stop therefore needs no
stale-owner inspection or path reclamation.

Remove PID liveness probes, start-time comparison, `ps`, `kill -0`, abandoned
text classification, reclaim deletion, and unconditional drop deletion. A
free legacy lock file is ordinary persistent metadata. After the operating-
system lock succeeds, overwrite it with the current diagnostic record.

Guarantee watch exclusion on supported Linux, macOS, and Windows local file
systems that provide the required operating-system lock behavior. A lock
operation error fails closed. Do not fall back to the PID-file protocol.
Network file systems without reliable operating-system file locks are
unsupported. An external program that deletes or replaces the persistent lock
file while criv holds it is outside this guarantee.

Do not promise safe concurrent watch operation between this protocol and an
older criv release that only uses PID-file ownership. The implementing release
notes require users to stop an older watcher before they start the new release.
They also state that `.criv/watch.lock` is persistent and must not be manually
deleted while criv runs.

Use real child processes and deterministic start barriers for concurrency
tests. The test gate proves:

- Two simultaneous first starts produce exactly one owner.
- Many simultaneous contenders produce exactly one owner.
- Live watch blocks live and one-shot watch.
- One-shot watch blocks live and one-shot watch.
- Normal release permits the next owner.
- Forced process termination permits immediate recovery on the same path.
- Old, malformed, and partial text never controls ownership.
- Diagnostic publication failure happens before refresh.
- The persistent file remains after release.
- A symbolic link, junction, directory, non-regular file, and path escape fail
  closed.
- Operating-system lock failure happens before refresh.

Run the real cross-process lock tests in required Linux, Windows, and macOS CI
lanes. Add a focused required macOS lock-test job; a release build without tests
is not verification of lock behavior. Keep portable format and error tests in
the complete workspace test suite.

## Consequences

Two cooperating criv processes cannot both own watch refresh for one
repository. Ownership no longer depends on a time window between file creation
and text publication, and a former owner cannot release a later owner lock.

Crash recovery is automatic because the operating system closes process file
handles. The persistent path can contain stale diagnostic text while it is
free, but that text has no behavioral effect.

The implementation needs a confined open-or-create helper and real
cross-process tests. macOS gains a focused hosted test cost. Network file
systems and concurrent older watch implementations remain explicit support
limits rather than unsafe recovery guesses.

The structural confinement policies in
[[0090-enforce-runtime-boundary-decisions|ADR-0090]] remain active. They prevent
watch code from bypassing the confined mutation helper, while behavioral tests
prove file-lock ordering and process recovery.

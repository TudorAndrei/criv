---
id: ADR-0166
kind: decision
title: Own Refresh Inside the Watch Generation
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0126
  - ADR-0151
governs:
  - src/watch.rs
  - src/watch/refresh.rs
  - src/watch/refresh/tests.rs
  - src/check.rs
  - src/repository.rs
  - src/lib.rs
  - src/discovery/mod.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/vault.rs
---

# Own Refresh Inside the Watch Generation

## Context

[[0126-own-one-source-state-per-refresh|ADR-0126]] gives one `SourceState` to
each refresh and makes `RefreshSession` keep its configuration and last
successful `RefreshResult`. [[0151-retain-watch-generation-ownership-in-standard-form|ADR-0151]]
puts the ordinary refresh checks in `ActiveWatchGeneration`.

`src/refresh.rs` had one caller, the watch module, and the two changed
together. It held test-only constructors, a test checkpoint field with two
hooks, and getters that only forwarded fields. The watch generation, the
refresh, and `src/state.rs` each held a part of the publication gate. The
watch module walked path components with ambient `symlink_metadata` and a
separate Windows junction probe, outside `RepositoryFiles`, which
[[0128-enforce-runtime-paths-through-repository-files|ADR-0128]] makes the
only path interface. It also wrote the watch-lock record text in one function
and parsed it by hand in another.

## Decision

Retain the complete Source-state decision of ADR-0126 and the generation
ownership of ADR-0151, with these changes:

- `RefreshSession`, `RefreshResult`, and `RefreshCause` live in the private
  `src/watch/refresh.rs` child of the watch module. `RefreshSession::new`
  takes the repository handle and the configuration; `criv watch --once`
  loads the configuration and calls it. `refresh` takes the cause and the
  precommit check. There is no check-free variant and no test hook.
- `check::validate_for_publication` is the one publication gate that the
  refresh calls. It rejects unresolved effective governance before any policy
  work, then returns the full validation. State keeps its own refusal of a
  failed registered policy definition (ADR-0162).
- Watch path kinds come from `RepositoryFiles::entry_kind`, which reads a path
  through the same no-follow capability opens as every other repository read.
  A link, a junction, a non-directory ancestor, or an unreadable entry has the
  kind `Other`.
- One watch generation refresh reads `criv.toml` before the commit and again
  after the attempt, as ADR-0151 requires. When the first read already shows
  a change, the second read does not occur. A failed read is a
  reconfiguration; the reconfiguration reads again and reports that failure.
- `WatchLockRecord` owns the record format of
  [[0095-operating-system-watch-session-lock|ADR-0095]]: it renders and parses
  the schema, PID, and mode.
- The poll interval, the debounce interval, and the reconfiguration retry
  interval are named constants. Their values do not change.

Tests use real behavior. A configuration that changes before the refresh
reaches the precommit check, and a corrupt snapshot makes a refresh fail.
Tests do not read the private transaction record.

## Consequences

A change to refresh, its gate, or its generation rules has one module. The
watch module no longer has an ambient path walk.

The deterministic tests cannot change the configuration at the exact commit
point. A change before the attempt reaches the same precommit check, and the
State publication tests cover the commit point with their controlled
file-system seam.

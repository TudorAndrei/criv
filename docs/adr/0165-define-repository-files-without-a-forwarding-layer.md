---
id: ADR-0165
kind: decision
title: Define Repository Files without a Forwarding Layer
status: accepted
date: 2026-11-06
governs:
  - src/repository.rs
  - src/repository/filesystem.rs
---

# Define Repository Files without a Forwarding Layer

## Context

[[0139-name-the-glob-module-for-what-it-holds|ADR-0139]] makes
`src/repository.rs` the only caller interface for repository files and
`src/repository/filesystem.rs` its private operating-system implementation.
[[0137-link-creation-guard-follows-the-helper|ADR-0137]] names
`RepositoryWriteScope::link_dir` as the only way to create a link.

`RepositoryFiles` and `RepositoryWriteScope` in `src/repository.rs` forwarded
each of their twenty methods to a private `FileSystem` type with the same
methods. Two methods existed only for one caller each: a test-only
`write_atomic_if_changed`, and `restore_with_lock` for the Git index snapshot.

The implementation also had these problems:

- Every directory open first checked each path component with an ambient
  `symlink_metadata`, but the no-follow capability open already refuses a
  link.
- A failed link creation always became `LinkOutcome::Unsupported`.
- A link that could not be read counted as a different link.
- The temporary-file cleanup ignored its own error.
- The lock open retried a `NotFound` error nine times with no stated reason.

## Decision

`src/repository/filesystem.rs` defines `RepositoryFiles` and
`RepositoryWriteScope`, and `src/repository.rs` re-exports them with the link
outcome types. The interface path, the private child, and
`RepositoryWriteScope::link_dir` stay as ADR-0139 and ADR-0137 decide. There
is no forwarding layer.

- The no-follow capability open is the only component check. When it refuses
  a component, the same directory handle describes the cause: a link or a
  non-directory.
- Only an unsupported or permission-denied link creation is
  `LinkOutcome::Unsupported`, which falls back to copies. Other errors return.
- A link read error returns. One helper decides whether two paths name the
  same physical directory.
- A failed cleanup is added to the error that caused it.
- `RepositoryWriteScope::create_new` creates a file exclusively. The ADR
  reconcile transaction uses it for `index.lock`.
- `open_regular_file` keeps its bounded retry. Two writers that create the
  same lock file at the same moment can get `NotFound` from the capability
  open, and `concurrent_writers_leave_one_complete_revision` fails without
  the retry.

## Consequences

A new file operation is written once. The interface does not change for
callers.

A link creation that fails for a reason other than platform support is an
error instead of a silent copy.

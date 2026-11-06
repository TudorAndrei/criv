---
id: ADR-0164
kind: decision
title: Type Git Revisions on One Opened Repository
status: accepted
date: 2026-11-06
governs:
  - src/git.rs
  - src/enforce.rs
  - src/check.rs
  - src/query.rs
---

# Type Git Revisions on One Opened Repository

## Context

[[0058-embedded-git-repository-access|ADR-0058]] makes `src/git.rs` the only
production Git boundary, and keeps the documented comparison fallbacks of
`criv enforce`.

The boundary had about twenty free functions. Each one discovered the
repository again from the root. The string `":"` meant the index, and an
absent new ref meant the worktree, so a caller could not tell a worktree
comparison from a pre-push set of several commits. Four `match` arms built the
same diff with their own error text. Reads returned an error for a missing
file, so callers used `.ok()` and could not tell a missing file from a failed
read. Errors named reconciliation and history fetches, which are the words of
one caller.

`criv enforce` had more of the same kind:

- a hidden `--remote-url` option that it never read;
- a manual push that dropped the `@{upstream}` error before it fell back to
  `HEAD~1`;
- a CI comparison that dropped both `GITHUB_BASE_REF` errors;
- a CI report that always gave zero changed files.

## Decision

`GitRepository` is opened once per operation, and every Git operation is a
method on it. `git::Revision` names one side of a comparison: `Commit`,
`Index`, or `Worktree`. A changed entry has a `Revision`; a changed set has
one, or none for pre-push input. `GitRepository::file` returns the bytes and
mode at a revision, `None` for a missing file, and an error for a failed read.
One `changed_set` method takes a `Comparison` and an optional path list.
Errors describe the Git operation; callers add their workflow words.

`criv enforce`:

- drops `--remote-url`;
- keeps the manual-push fallback to `HEAD~1`, and puts the reason in the
  comparison basis;
- names each failed `GITHUB_BASE_REF` comparison in its error, or in the
  basis of the worktree fallback outside CI;
- reports the changed file count in CI. CI still scans every file.

## Consequences

A Git read has one place to change. A missing file and a failed read are
different values.

An installed pre-push hook that passes `--remote-url` fails until it is
regenerated. criv has not generated that argument since its embedded pre-push
mode.

The CI JSON report gives the real `changed_files` count.

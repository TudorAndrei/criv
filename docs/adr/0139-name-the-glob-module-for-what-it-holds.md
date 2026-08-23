---
id: ADR-0139
kind: decision
title: Name the Glob Module for What It Holds
status: accepted
date: 2026-08-23
supersedes:
  - ADR-0136
governs:
  - src/glob.rs
  - src/identity.rs
  - src/markdown.rs
---

# Name the Glob Module for What It Holds

## Context

[[0136-retire-the-util-module|ADR-0136]] split `src/util.rs` by concern and
moved note parsing to `src/markdown.rs` and name conversion to
`src/identity.rs`. It left `GlobMatcher` in a file still called `util.rs`, and
gave this reason: renaming it would cost the governed scopes of three accepted
decisions, which was more than a name was worth.

That reason was wrong, and it inverted the test. A module is kept or retired on
whether it gives a caller leverage, never on how much administrative work the
change implies. `util` named no concern, so a caller who learned it existed
learned nothing. That was true of the seven-item grab bag and it stayed true of
the one-item file.

Governance cost is an argument for using the reconciliation path, not an
argument for keeping a bad name. criv has that path, and it worked here: once
`util.rs` held `GlobMatcher` alone, `util.rs` to `glob.rs` was a one-to-one
rename that Git detects at full similarity, and
`criv adr reconcile-sources --base HEAD~1` rewrote the governed scopes of
[[0003-adopt-proven-foundation-crates|ADR-0003]],
[[0095-operating-system-watch-session-lock|ADR-0095]], and
[[0127-own-repository-files-behind-one-interface|ADR-0127]] against a receipt.
No decision was retired to do it.

## Decision

`src/glob.rs` holds `GlobMatcher`, and its name says so. The rest of
ADR-0136 stands: `src/markdown.rs` owns the two note-body parsers, and
`src/identity.rs` owns `kebab`, `is_adr_id`, `strip_prefix`, and the test-only
fixture copier.

Retire a module when it gives no leverage. Reconcile the governed scopes with
`criv adr reconcile-sources` when the move is a rename, and write a successor
decision when it is a deletion. Never keep a name because moving it is
inconvenient.

## Consequences

Three modules of about 104, 48, and 126 lines replace one file of 274 that
answered no question about its contents. Each name tells a caller what is
inside.

The prose of the three reconciled decisions still says `src/util.rs`, because
[[0012-adr-immutability-enforcement|ADR-0012]] keeps an accepted decision's text
as written. Only the `governs:` scopes moved, which is the part `criv check`
resolves.

A future rename of a governed source file has a worked precedent: reduce the
file to the concern that survives, rename it one-to-one, and reconcile against
the commit before the rename rather than against the integration target, so the
mapping stays unambiguous.

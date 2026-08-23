---
id: ADR-0139
kind: decision
title: Name the Glob Module for What It Holds
status: accepted
date: 2026-08-23
supersedes:
  - ADR-0003
  - ADR-0095
  - ADR-0127
  - ADR-0136
governs:
  - src/config.rs
  - src/glob.rs
  - src/identity.rs
  - src/lib.rs
  - src/markdown.rs
  - src/repository.rs
  - src/repository/filesystem.rs
  - src/state.rs
  - src/vault.rs
  - src/watch.rs
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
the one-item file. Writing a decision is cheap; a name that teaches nothing is
paid for on every read.

Reconciliation cannot absorb this move. `criv adr reconcile-sources` compares
two points, so against the integration target `src/util.rs` is deleted and three
files are added, not renamed. criv's rule for a deleted governed path is a
successor decision, and this is it.

## Decision

`src/glob.rs` holds `GlobMatcher`, and its name says so. `src/markdown.rs` owns
the two note-body parsers. `src/identity.rs` owns `kebab`, `is_adr_id`,
`strip_prefix`, and the test-only fixture copier. `src/util.rs` is gone.

Retire a module when it gives no leverage. Reconcile with
`criv adr reconcile-sources` when the move is a one-to-one rename, and write a
successor decision when it is a split or a deletion. Never keep a name because
moving it is inconvenient.

This decision carries forward what the three superseded decisions still decide,
so nothing is lost by retiring them.

### From ADR-0003, foundation crates

Use focused crates for foundational behavior before growing heavier backends:
`thiserror` for errors, `globset` for glob matching, `pulldown-cmark` for
Markdown event parsing, `content_inspector` for text and binary classification,
`mime_guess` for extension MIME hints, `serde_norway` for YAML frontmatter,
`blake3` for stable hashes, and `notify-debouncer-mini` for watcher debouncing.

`clap` is no longer among them. The CLI parses with `usage-rs` under
[[0134-parse-the-cli-with-usage|ADR-0134]], which narrowed ADR-0003 before this
decision superseded it.

The implementation surfaces are `src/lib.rs`, `src/config.rs`, `src/glob.rs`,
`src/vault.rs`, `src/state.rs`, and `src/watch.rs`.

### From ADR-0095, the watch session lock

Keep two lock roles. A watch-session lock permits one live or one-shot refresh
owner for the full session. A State publication lock protects one disk
publication or local snapshot read. A live watcher or one-shot run takes the
watch-session lock first, then the publication lock for each automatic
publication. `query diff` snapshot lookup takes only the publication lock. The
order is fixed: never take the watch-session lock while holding the publication
lock.

`WatchSessionLock` owns confined file open and creation, the operating-system
lock, diagnostic publication, contention diagnostics, and release.
`LiveWatchSession` holds one guard for its lifetime, and `watch --once` holds
one for its refresh. Acquire the guard before configuration load, session
construction, State publication, or generated architecture mutation.

`.criv/watch.lock` stays one persistent regular file below the selected
repository's `.criv/` directory, opened through the confined helper that rejects
a symbolic link, junction, directory, non-regular file, path escape, or
non-real parent, as [[0044-vault-write-confinement|ADR-0044]] requires. Never
replace, rename, or delete it during a session.

### From ADR-0127, repository files behind one interface

`src/repository.rs` is the only caller interface for repository file access, and
`src/repository/filesystem.rs` is its private operating-system implementation.
Non-I/O helpers stay outside it: glob matching in `src/glob.rs`, Markdown
parsing in `src/markdown.rs`, name conversion in `src/identity.rs`.

`RepositoryFiles` opens one explicit repository root and keeps its canonical
display path and capability directory handle together. `RepositoryWriteScope`
comes only from `RepositoryFiles::write_scope`, binding one validated allowed
directory to the same handle before any mutation.

A command, transaction, or refresh generation opens one `RepositoryFiles` value
and passes a reference through its owners rather than reopening the root per
file. State publication, ADR reconciliation, Init, installation, and watch
locking keep one value for the complete operation. A new live-watch
configuration generation opens a new value.

[[0128-enforce-runtime-paths-through-repository-files|ADR-0128]] and
[[0138-confine-repository-reads|ADR-0138]] hold the policy patterns that enforce
this, and both stay effective. Retiring ADR-0127 removes no guard.

## Consequences

Three modules of about 104, 48, and 126 lines replace one file of 274 that
answered no question about its contents. Each name tells a caller what is
inside.

ADR-0003, ADR-0095, and ADR-0127 become historical. None of them carried a
policy pattern, so no enforcement changes. Their governed scopes move to this
decision, which is why its `governs:` list is the union of theirs with
`src/util.rs` replaced by the three modules that took its contents.

A future rename of a governed source file has a worked precedent. Reduce the
file to the concern that survives and rename it one-to-one, then reconcile
against the commit before the rename. Where the move is a split, write the
successor decision instead and carry forward what the retired decisions still
decide.

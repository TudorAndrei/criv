---
id: ADR-0127
kind: decision
title: Own Repository Files Behind One Interface
status: accepted
date: 2026-08-21
governs:
  - src/lib.rs
  - src/glob.rs
  - src/config.rs
  - src/check.rs
  - src/discovery/mod.rs
  - src/source.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/structural.rs
  - src/vault.rs
  - src/watch.rs
  - src/init.rs
  - src/install/skills.rs
  - src/adr.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
  - src/state/publication.rs
  - src/state/snapshots.rs
  - src/git.rs
---

# Own Repository Files Behind One Interface

## Context

[[0044-vault-write-confinement|ADR-0044]] defines the visible repository path
rules. Repository paths are relative, cannot contain parent traversal, and
cannot pass through links. Writes stay inside an explicit allowed directory
and publish atomically.

The implementation now prevents path-replacement races by keeping no-follow
directory handles through final reads and mutations. However, the interface is
still split. `src/util.rs` owns many free functions and the private
`ConfinedFile` implementation. `src/source/paths.rs` adds another relative path
check. `src/discovery/mod.rs` owns selected-file checks.
`src/adr/reconcile_transaction.rs`, `src/state/publication.rs`, and
`src/state/snapshots.rs` combine the free functions into separate file
lifecycles.

Every caller passes a root path for each operation. Mutation callers also pass
an allowed directory and a destination each time. The types do not bind these
values into one repository observation or one write scope. A caller must learn
which group of functions preserves confinement, regular-file checks,
permissions, atomicity, rename ordering, removal, and directory durability.

[[0105-owner-scoped-rust-module-layout|ADR-0105]] requires one parent interface
when several files implement one owner. The repository file implementation is
a local dependency with one production implementation. Temporary repositories
already provide its real test environment.

## Decision

Create one crate-local Repository Files module. Put its interface in
`src/repository.rs` and its private operating-system implementation in
`src/repository/filesystem.rs`. Do not keep forwarding filesystem functions in
`src/util.rs`. Keep Markdown parsing, glob matching, name conversion, and other
non-I/O helpers in `src/util.rs`.

The parent interface owns two values:

- `RepositoryFiles` opens one explicit repository root and keeps its canonical
  display path and capability directory handle together.
- `RepositoryWriteScope` comes only from `RepositoryFiles::write_scope`. It
  binds one validated allowed directory to the same repository handle before
  any mutation.

A command, transaction, or refresh generation opens one `RepositoryFiles`
value and passes a reference through its owners. It does not reopen the root
for each file. State publication, ADR reconciliation, Init, installation, and
watch locking keep one value for the complete operation that must observe one
repository root. A new live-watch configuration generation opens a new value.

### Repository file interface

`RepositoryFiles` exposes repository-relative reads and observations:

- required and optional regular-file reads as bytes or UTF-8 text;
- reads that also return permissions or metadata when the caller must restore
  or inspect them;
- regular-file and directory existence checks;
- directory entry names.

`RepositoryWriteScope` exposes mutations within its bound allowed directory:

- create a directory and its missing parents;
- write a new file;
- append a line when it is absent;
- atomically write text or bytes, with optional preserved permissions;
- rename or remove a regular file;
- remove an empty directory; and
- open one persistent regular file for the operating-system watch lock; and
- create, replace, or remove the supported generated directory link.

Destinations remain repository-relative, even on a write scope. The scope
checks that every destination is below its allowed directory. A rename checks
both paths against the same scope. The interface does not expose
`cap_std::fs::Dir`, temporary-file names, opened parent handles, or a method
that separates validation from the final operation.

Keep the visible path and error behavior from ADR-0044. Reject empty, absolute,
rooted, prefixed, and parent-relative paths. Reject linked intermediate
components, linked final files, non-regular file reads, and mutations outside
the allowed directory. Keep narrow write scopes for generated artifacts and
allow `.` only where the command already owns repository-wide writes.

### Platform implementation

Use `cap_std::fs::Dir` and `cap-fs-ext` for the common implementation. Open the
root once. Traverse every normal path component with handle-relative,
no-follow operations. Keep the opened parent handle through the final open,
temporary file, rename, permission change, removal, and directory sync.

Path-based normalization and link checks can remain as early diagnostics, but
they are not the confinement authority. Do not add a path-based fallback for a
security-sensitive operation.

Keep the current platform rules:

- Unix uses a relative directory symlink for the generated skill link and
  syncs changed directories.
- Windows uses a directory junction for the generated skill link. Directory
  sync remains best effort where the platform does not support it.
- Other platforms report that generated links are unsupported and use the
  same regular-file confinement implementation.

Keep link creation as a private part of the repository filesystem
implementation. Its platform branches do not become caller interfaces.

### Ownership limits

Repository Files owns safe file access, not file meaning.

- Discovery keeps ignore rules, explicit roots, include and exclude rules,
  hidden-file rules, binary selection, and selected-link diagnostics from
  [[0111-file-discovery-compatibility-contract|ADR-0111]]. Its final selected
  reads use Repository Files. Discovery link checks describe selection
  behavior; they are not a second confinement implementation.
- Source keeps lossy UTF-8 conversion and Source parse errors. Remove its
  duplicate relative-path validation. `src/source/paths.rs` may keep only
  Source-specific conversion if that implementation still earns a private
  child; it must not own no-follow traversal.
- State publication keeps transaction planning, checkpoints, rollback, and
  recovery. Rename its test-only `PublicationFileSystem` control so it states
  that it injects transaction checkpoints, not a filesystem adapter. All tests
  use the real Repository Files implementation.
- Git history, diffs, references, and index meaning stay behind `src/git.rs`
  under [[0058-embedded-git-repository-access|ADR-0058]]. Raw index-file backup
  can use Repository Files without moving Git semantics into this module.
- Callers keep their domain-specific error context. Repository Files returns
  the confined path error; an owner can add the operation name.

This is a local-substitutable module with one production implementation. Do not
add a filesystem trait, port, mock, in-memory adapter, or caller-selected
backend. Test through the parent interface with temporary repositories and
real operating-system files.

### Compatibility

This decision does not change command options, CLI text, repository path
rules, file formats, State schema, snapshot schema, graph-cache schema,
transaction order, rollback behavior, permissions, discovery results, Source
text handling, editor behavior, or link-or-copy fallback.

The path-replacement defect was fixed before this migration. The migration
must preserve its held-handle tests and must not replace the working security
fix with only path checks.

## Migration

1. Add interface tests for required and optional reads, regular-file checks,
   scope escape, atomic replacement, permission round trips, rename, removal,
   directory creation, link behavior, final-link replacement, and intermediate
   path replacement. Run the Windows junction cases in hosted Windows
   validation.
2. Add `src/repository.rs` and private
   `src/repository/filesystem.rs`. Move `ConfinedFile`, capability traversal,
   atomic temporary-file handling, directory sync, link handling, and path
   validation behind the parent interface.
3. Thread one `RepositoryFiles` value through Source refresh, Vault loading,
   State publication, ADR reconciliation, Init and generated-skill
   installation, watch locking, and the embedded Git index backup. Keep one
   value for each complete operation.
4. Replace final selected reads in Discovery and Source. Remove duplicate
   Source path validation, while keeping discovery selection rules unchanged.
5. Remove every production repository I/O function and capability import from
   `src/util.rs`. Move test-only fixture copying to test support or leave raw
   filesystem setup inside test modules.
6. Rename the State publication checkpoint control and keep its failure tests
   on the real Repository Files interface. Do not add a second adapter.
7. Add a new path-update ADR that supersedes the active runtime-policy owner,
   updates the confinement policy message from `src/util.rs` to
   `src/repository.rs`, and keeps the private filesystem child outside the raw
   mutation policy scope.
8. Update the Component and Code architecture map. Show one Repository Files
   component, `criv::repository` as the parent Code interface, and
   `criv::repository::filesystem` as its private implementation. Show all
   callers using the parent. Validate and render every changed view.
9. Run the complete Rust, Windows, vault, and architecture verification gates.
   Compare serialized fixtures and public CLI output with the pre-migration
   results.

## Consequences

Callers learn one root-bound interface and one bound write scope instead of a
list of free functions and repeated root and scope parameters. A root handle
cannot be mixed with a destination from another repository. A write scope
cannot be mixed with a different allowed directory.

Confinement, regular-file checks, atomicity, permissions, rename, removal,
link behavior, and directory durability become local to one implementation.
A correction applies to Source, State, ADR reconciliation, Init, installation,
watch, and Git index backup through the same interface.

Deleting Repository Files would move capability traversal and mutation safety
back into every caller. The module therefore passes the deletion test.

The migration is large and safety-critical. It changes ownership and types,
not visible path policy or file behavior.

## Alternatives Considered

### Keep free functions in util

Rejected. The generic module name hides a security-critical owner. Repeated
root, allowed-directory, and destination parameters leave correlation to each
caller.

### Add one filesystem wrapper inside every domain owner

Rejected. Source, State, Governance, and Installation would each learn and
test the same path and no-follow rules. A security fix would again need several
implementations.

### Add a filesystem trait and mock

Rejected. There is one production implementation. A mock would test the mock's
file behavior and add a hypothetical seam. Temporary repositories exercise the
real interface on every supported operating system.

### Put Git history under Repository Files

Rejected. Git graph and history semantics form a separate deep module with a
different dependency. Repository Files can provide raw confined bytes without
absorbing `git2` values or Git behavior.

### Rely on canonical paths and repeated link checks

Rejected. Another process can replace a checked path before a later path-based
operation. Held capability handles are the confinement authority.

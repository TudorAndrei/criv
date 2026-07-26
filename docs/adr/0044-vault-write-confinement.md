---
id: ADR-0044
kind: decision
title: Vault Write Confinement
status: accepted
date: 2026-07-25
governs:
  - src/util.rs
  - src/check.rs
  - src/state.rs
  - src/architecture.rs
  - src/source_graph.rs
  - src/init.rs
---

# Vault Write Confinement

## Context

criv writes files into repositories it does not own: generated state and
snapshots, the generated code architecture artifact, the durable graph cache,
`criv init` scaffolding, and Markdown rewritten by `criv check --fix`. Those
writes run inside Git hooks and CI, against repository content that can arrive
through a clone or a pull request.

The 2026-06 audit remediation added confinement helpers in
`src/util.rs` so generated writes validate their destination, refuse
absolute and parent-relative paths, reject symlink components before and after
directory creation, and publish atomically through a temporary file and rename.
That hardening was never recorded as a decision, so the invariant lived only in
the implementation and its call sites.

The 2026-07-25 audit then found the gap that omission allowed. The Markdown fix
pass in `src/check.rs#fn:apply_markdown_fixes` inherited the vault docs
directory as its allowed write directory, while `criv check` deliberately lints
Markdown across the whole repository. Any fixable file outside `docs/` aborted
the entire command with a confinement error and printed no diagnostics at all.
The guard was correct; the scope it was given did not match the command it was
guarding.

[[0021-audit-remediation-boundaries|ADR-0021]] established that user-facing
flags should correspond to active behavior. A `--fix` flag that lints a file it
will never rewrite is the surface-without-behavior shape that decision argued
against.

## Decision

Every criv write into a repository goes through the confinement helpers in
`src/util.rs`. Callers pass a root, an allowed directory, and a
repository-relative destination together, so a caller cannot validate one path
and then write another. Writes never follow symlinks, never escape the
repository root, and are published atomically.

The allowed directory expresses the write's scope, not its safety. Root
confinement, symlink rejection, and relative-path validation apply at every
allowed directory, including `.`. Narrowing the allowed directory to a
subdirectory is a scoping decision and must be chosen per command rather than
inherited from an unrelated one.

Refusing to write through a symlinked vault path is correct behavior, not only a
safety measure. [[0002-docs-and-adrs-form-the-governance-graph|ADR-0002]] models
`docs/` as the committed vault, and a vault's notes govern the source that sits
beside them. Documentation that lives outside the repository and is symlinked in
is not versioned with the code it governs, so its history, review, and
enforcement no longer track the project it describes. A symlinked vault path is
therefore out of scope for criv, and a command that meets one should fail rather
than resolve it.

`criv check --fix` rewrites any Markdown file it lints, anywhere inside the
repository root. Which files criv lints is controlled by the rumdl
configuration's include and exclude lists, which is where an operator expresses
that a directory is not vault content. The fix pass must not apply a second,
narrower scope that silently declines to fix files the same command reports
diagnostics for.

Generated artifacts keep their narrow scopes: state and snapshots stay under
`.criv`, and the generated code architecture stays under the configured vault
docs directory.

## Consequences

The confinement invariant is now a recorded decision rather than a property of
four call sites, so a future change that adds a write path has a rule to comply
with instead of an example to copy.

`criv check --fix` becomes able to rewrite repository-root Markdown such as
`README.md`. Operators who do not want a directory touched exclude it in rumdl
configuration, the same mechanism that already controls what is linted.

`criv init` must be brought under this rule. At the time of this decision its
template, hook, and `.gitignore` writes tested path existence and then wrote
through the standard library, so they followed symlinks, and the hook path
additionally marked the resolved target executable. Routing every one of those
writes through the confinement helpers is required by this decision, not
optional follow-up work: `criv init` is the tool's first filesystem contact with
a repository criv does not yet know, which makes it the write path that most
needs the guard.

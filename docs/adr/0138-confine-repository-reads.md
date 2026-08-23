---
id: ADR-0138
kind: decision
title: Confine Repository Reads
status: accepted
date: 2026-08-23
governs:
  - src/adr.rs
  - src/adr/source_reconcile.rs
  - src/check.rs
  - src/init.rs
  - src/state.rs
  - src/vault.rs
policy:
  patterns:
    - id: confined-repository-reads-only
      language: rust
      rule: |
        all:
          - any:
              - pattern: std::fs::read_to_string($$$ARGS)
              - pattern: fs::read_to_string($$$ARGS)
              - pattern: std::fs::read($$$ARGS)
              - pattern: fs::read($$$ARGS)
              - pattern: File::open($$$ARGS)
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: Read repository files through RepositoryFiles, which rejects symlinked components and confines the path to the repository root.
---

# Confine Repository Reads

## Context

[[0128-enforce-runtime-paths-through-repository-files|ADR-0128]] made
`src/repository.rs` the only caller interface for repository file access, and
guarded the decision with the `confined-repository-mutations-only` policy.

That pattern matches only mutations: `write`, `rename`, `remove_file`,
`remove_dir_all`, `create_dir_all`, `File::create`, and `OpenOptions::new`.
Reads were invisible to the check meant to enforce the decision.

The reads had drifted. `criv adr reconcile` read the contents of every
Git-tracked path with ambient `fs::read_to_string`, and both `is_file()` and the
read follow symlinks. A tracked symlink pointing outside the repository was read
into memory, and if it matched an ADR mapping its content was hashed into
`.criv/adr-reconcile.json`. Writes stayed confined, so this was an
out-of-confinement read and a metadata leak rather than an escape, but the whole
point of [[0044-vault-write-confinement|ADR-0044]] and ADR-0128 is that reads
and writes share one capability root.

`worktree_file_mode` in the same module already rejected symlinks, which shows
the intent was there and was applied unevenly.

## Decision

Route repository reads through `RepositoryFiles`, the same as mutations.
`read_string`, `read`, `read_optional_string`, and `file_exists` reject
symlinked path components and confine the path to the repository root.

Guard reads with their own policy pattern, `confined-repository-reads-only`,
alongside ADR-0128's mutation pattern. This decision adds a rule; it does not
replace ADR-0128, which stays correct about everything it decided.

Exclude `mod tests` from the pattern, as ADR-0128 does. A fixture that reads a
temporary directory it just built is not a confinement question.

## Consequences

`criv adr reconcile` now refuses a tracked symlink that leaves the repository,
where it previously read through it. A repository that relies on such a symlink
sees reconciliation fail with the confinement error rather than silently hashing
outside content.

The confined reader is stricter in two more ways: it errors on non-UTF-8 where
the ambient reader returned bytes, and on non-regular files. `rewrite_candidates`
already handled the non-UTF-8 case explicitly, so its behaviour is unchanged.

A new ambient read in a governed module now fails `criv check` and
`criv enforce`, naming this decision.

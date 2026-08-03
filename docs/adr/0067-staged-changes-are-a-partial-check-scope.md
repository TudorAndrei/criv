---
id: ADR-0067
kind: decision
title: Staged Changes Are A Partial Check Scope
status: accepted
date: 2026-08-03
governs:
  - src/check.rs
  - src/git.rs
  - src/enforce.rs
  - hk.pkl
targets:
  symbols:
    - src/enforce.rs#fn:changed_entries
    - src/git.rs#type:ChangedSet
---

# Staged Changes Are A Partial Check Scope

## Context

`criv check` validates the complete repository Markdown set and loads a complete
vault before it applies the output-only `--filter`. In this repository the hk
step in `hk.pkl` runs that full path whenever any Markdown file, `criv.toml`, or
`.rumdl.toml` changes. That is the correct authority for hosted validation, but
it repeats work during pre-commit even when the staged transaction affects one
input whose checks are local.

`enforce::changed_entries` already selects fail-closed Git change sets for
enforcement, and `git::ChangedSet` represents additions,
modifications, renames, deletions, and both sides of a comparison. Adding a
separate filesystem walk for check scoping would create a second definition of
the staged transaction and could disagree with commit enforcement.

Not every check can be scoped to changed files. Duplicate note or pattern
identities, supersession consistency and cycles, inbound and ambiguous link
resolution, and repository-wide topology properties depend on unchanged
inputs. A changed source can also invalidate a reference in an unchanged note.
Reporting those properties from a subset would make a fast command appear more
authoritative than its evidence.

GitHub issue #21 requires an opt-in fast path without replacing the full check
that owns the CI gate.

## Decision

Add `criv check --changed` as a read-only partial validation mode over the
staged Git transaction. It uses the same embedded Git repository and changed
entry representation as commit enforcement. Rename entries contribute both
their old and new paths. Failure to discover a worktree, inspect the index, or
represent a changed path is an error; the command does not fall back to an
unscoped filesystem guess.

The changed mode evaluates only facts whose evidence can be restricted to the
staged inputs:

- rumdl formatting for staged Markdown files that still exist;
- file-local note metadata and C4 syntax for staged vault files;
- outgoing links, source targets, policy references, and C4 anchors authored
  by a staged note or artifact, resolved against the complete loaded vault;
- policy scans whose owner scopes intersect staged source paths.

The mode does not report repository-global validity. Global identity,
supersession, inbound-link, ambiguity, orphan, and topology analyses remain the
responsibility of plain `criv check`. A staged deletion or rename, an ADR
change, or a change to `criv.toml` or `.rumdl.toml` can alter global identity,
scope, configuration, or file selection; `--changed` therefore promotes those
transactions to the existing full check rather than returning a partial result.
An empty staged set succeeds without loading a vault.

`--changed` and `--fix` are mutually exclusive. Fixing is intentionally based
on rumdl's complete configured file set under
[[0044-vault-write-confinement|ADR-0044]], while changed mode is a read-only
latency optimization. `--format` and `--filter` retain their existing rendering
and exit-status behavior over whichever diagnostics the selected mode
produces. Machine-readable formats contain diagnostics only; they do not gain a
partial-result wrapper.

The repository's pre-commit check step uses `criv check --changed`. The hk
`check` profile, hosted CI annotation command, manual plain `criv check`, and
CI-stage enforcement continue to use full validation. This refines the lean
local-hook boundary in
[[0060-parallel-hosted-validation-and-lean-local-hooks|ADR-0060]] and
[[0061-hook-owned-local-validation-and-direct-ci-profile|ADR-0061]] without
changing the hosted authority.

## Consequences

Small staged Markdown or source edits avoid unrelated Markdown lint and policy
scan work during pre-commit. The command still loads enough complete vault data
to resolve local references correctly, so its cost is not proportional in every
subsystem.

A passing changed check means that the safely scoped facts for the staged
transaction passed. It does not mean the complete vault is valid. Contributors
and automation use plain `criv check` when they need that claim, and hosted CI
continues to prove it on every change.

Conservatively promoting globally sensitive transactions sacrifices speed for
correctness. Future validation kinds must choose explicitly between changed
input scope, full-check promotion, and full-only execution; they must not be
silently omitted from both modes.

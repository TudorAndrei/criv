---
id: ADR-0177
kind: decision
title: Register Policy Patterns Once in the Vault
status: accepted
date: 2026-11-06
governs:
  - src/vault.rs
  - src/policy_scan.rs
  - src/structural.rs
  - src/check.rs
  - src/glob.rs
  - src/state/partitions.rs
---

# Register Policy Patterns Once in the Vault

## Context

[[0056-adr-policy-patterns-are-the-only-registered-patterns|ADR-0056]] makes
inline ADR policies the only named patterns.
[[0162-keep-the-vault-model-deep-and-state-fingerprints-in-state|ADR-0162]]
keeps the vault model deep and gives the policy plan plain data.

Four places decided which declaration owns a full pattern id. The vault
resolved a pattern through the first note with the ADR id, but its decision
map kept the last one. The policy plan checked a registered set and then
compared owner paths to drop the declarations the resolver did not pick. The
collision check counted every declaration, so a duplicate inside one note
reported `a.md, a.md`. The plan also scanned the duplicate declarations, so
each duplicate gave its own violations.

Notes could also declare `targets.patterns[].id`, a doc-local pattern id that
nothing resolved, matched, or published; its only effect was the
`duplicate-doc-pattern` diagnostic. The `targets: [list]` form was a legacy
spelling of `targets.symbols`.

State partitions matched changed files against the raw `governs` globs, so a
file governed through a Source selector was never scanned again. An invalid glob
matched nothing without a report.

## Decision

`Vault::load` indexes the policy patterns of the decision notes once. The
first declaration of a full `ADR-NNNN/local-id`, in note and declaration
order, owns that id; the decision map also keeps the first note of a
repeated id. `resolve_policy_pattern`, the policy plan, and the collision
check read this one index. A declaration is registered when it owns its id
and its note is an effective decision. The plan compiles every declaration
for its diagnostics and scans only the registered ones.

A repeated id inside one note is `duplicate-policy-pattern`. A full id that
more than one note declares is `duplicate-pattern-id`, reported once for each
note.

`targets.patterns[]` entries take only a required `ref`. The doc-local `id`,
the `duplicate-doc-pattern` code, and the `targets: [list]` form are removed;
such frontmatter no longer parses.

The vault owns `PolicyPattern` as plain frontmatter data with its language as
written. `src/structural.rs` takes a borrowed `PolicyDefinition`, parses the
language when it compiles, and reports any compile failure as one `Invalid`
error. `find_policies_batch` returns the matches of each request in request
order and reads only the requested paths.

`Vault::source_files_per_glob` is the one selection of Source files by
`governs` or `targets.scope` globs, with the Source-selector fallback. Policy
scans, State partitions, ADR source reconciliation, and check read it. A
policy partition is scanned again when a changed file is among its owner's
governed files or when a reused match is no longer governed. An invalid glob
is `invalid-glob`.

## Consequences

A duplicated pattern id gives one set of violations and one State partition,
and the State reuse rule cannot see the same id twice. A changed file that a
selector governs refreshes its State policy matches.

Vaults that used doc-local pattern ids or the list form of `targets` must
remove the ids and move the list under `targets.symbols`.

An invalid `targets.scope` glob is now an error instead of an
`empty-target-scope` warning.

---
id: ADR-0046
kind: decision
title: No Native Linting In criv enforce
status: accepted
date: 2026-07-30
supersedes:
  - ADR-0024
governs:
  - src/enforce.rs
---

# No Native Linting In criv enforce

## Context

[[0024-oxlint-only-javascript-typescript-enforcement|ADR-0024]] settled which
JavaScript and TypeScript linter `criv enforce` should invoke. It answered the
narrow question it was asked — oxlint, not ESLint — but it left the wider
question untouched: whether `criv enforce` should be running language linters at
all.

criv is a documentation-to-code knowledge graph validator, established by
[[0001-local-cli-vault-architecture|ADR-0001]]. Its enforcement stage exists to
check that documentation, ADR metadata, source references, and ADR-owned policy
patterns still match the code. Shelling out to a general-purpose linter is a
different job, and one the surrounding toolchain already owns: `hk.pkl` wires
`plugin-lint`, `vscode-lint`, and `actionlint` into the same hooks, and CI runs
`mise run check`.

The linting that grew inside `src/enforce.rs` had three specific problems.

It was never fully decided. `run_native_tools` also invoked `ruff` for Python
files, and no ADR ever mentioned ruff. Half of the behavior was governed by
ADR-0024 and the other half was undeliberated.

Its coverage was arbitrary. `src/source_graph.rs` indexes Rust, TypeScript,
JavaScript, Python, and Go. Enforcement linted two of those five and never linted
Rust, which is this repository's own primary language. Nothing principled decided
that set; it is the set someone happened to add.

It made criv's exit code depend on a tool criv does not configure. In a
downstream vault, `criv enforce` ran `ruff check` from the vault root over its
own file list, ignoring whatever invocation that project actually uses — its
per-package configuration, its selected rule sets, its `--fix` conventions — and
folded the result into the exit status that gates commits and pushes through the
generated hooks. A vault got a lint run it never asked criv for.

ADR-0024 argued that keeping a linter criv does not actually use "makes the
repository policy look ambiguous and produces misleading skipped-tool output".
That reasoning is sound and it generalizes further than ADR-0024 applied it.

## Decision

`criv enforce` runs no native language linters. Remove the oxlint and ruff
invocations, the package-local binary resolution that supports them, and the
optional-tool machinery in `src/enforce.rs` entirely.

Enforcement covers documentation and policy only: vault validation, ADR
immutability, import policy, and ADR-owned `policy.patterns` evaluated through
ast-grep per
[[0005-ast-grep-policy-search-and-enforcement|ADR-0005]] and
[[0041-adr-owned-policy-patterns|ADR-0041]].

Linting a project's source is the project's own responsibility, expressed through
its formatter and linter configuration and its hook runner. criv does not
duplicate, wrap, or gate it.

This supersedes ADR-0024. The question ADR-0024 answered — which JavaScript and
TypeScript linter criv should invoke — no longer applies, because criv invokes
none.

## Consequences

`criv enforce` no longer reports `Oxlint:` or `Ruff:` lines, and no longer fails
with `native enforcement tool(s) failed`. Its exit code now reflects only
documentation and policy state, which is what the command name has always
promised.

This repository's actual lint coverage is unchanged. The Obsidian plugin and the
VS Code extension keep oxlint as a pinned devDependency and keep their `npm run
lint` scripts; `hk.pkl` keeps running them through `plugin-lint` and
`vscode-lint` in the same hooks. What disappears is a second, weaker invocation
path, not the linting.

Downstream vaults keep whatever linting they already had. criv stops adding an
uninvited one.

The failure mode described in the 2026-07-25 audit — a missing binary reporting
success, so enforcement silently linted nothing while passing — is removed rather
than repaired, along with the package-local resolution added to repair it.

If native tool orchestration is ever wanted again, it must be config-driven and
declared by the vault, not hardcoded to a language list inside criv, and it needs
a new ADR because this decision is deliberate rather than incidental.

---
id: ADR-0054
kind: decision
title: Criv Does Not Install Git Hooks
status: accepted
date: 2026-07-31
governs:
  - src/init.rs
  - src/init/templates.rs
policy:
  patterns:
    - id: no-hookspath-ownership
      language: rust
      pattern: '"core.hooksPath"'
      message: criv neither reads nor writes core.hooksPath; a hook runner owns it.
    - id: no-generated-git-hooks
      language: rust
      pattern: '".githooks"'
      message: criv does not generate git hooks; document integration in docs/tooling.md instead.
---

# Criv Does Not Install Git Hooks

## Context

`criv init` wrote `.githooks/pre-commit` and `.githooks/pre-push` and pointed
`core.hooksPath` at that directory. No ADR ever decided this. It was described in
`README.md` but never governed, in the same way the `ruff` invocation removed by
[[0046-no-native-linting-in-criv-enforce|ADR-0046]] was never governed.

Setting `core.hooksPath` does not add hooks alongside whatever a repository
already uses. It **replaces** the hook directory entirely, so any hook runner
that installs into `.git/hooks` stops running, silently and with no diagnostic.

This repository is the proof. [[0013-mise-managed-hk-hook-toolchain|ADR-0013]]
gives hk ownership of hook behavior, and `hk install --mise` had written
`.git/hooks/pre-commit`. `criv init` then set `core.hooksPath = .githooks`, so
git never looked at `.git/hooks` again. Every hk `pre-commit` step — `cargo-fmt`,
`toml-fmt`, `actionlint`, `zizmor`, `vscode-lint`, `vscode-json-diagnostics` —
and the `commit-msg` conventional-commit check stopped running at commit time.
Nobody noticed, because `mise run check` and CI invoke hk directly and were
unaffected.

The failure surfaced only when unformatted Rust reached CI: a commit that hk's
`pre-commit` would have auto-formatted was accepted locally, because hk's
`pre-commit` was dead.

criv is not a hook runner. It is one of the commands a hook runner should call.
Owning `core.hooksPath` to guarantee itself a slot takes a repository-wide
setting away from the tool whose actual job that is.

## Decision

`criv init` does not create git hooks and does not read or write
`core.hooksPath`. The `--no-hooks` and `--force-hooks` flags are removed, because
[[0021-audit-remediation-boundaries|ADR-0021]] requires user-facing flags to
correspond to active behavior and these would control nothing.

Integrating criv into a hook runner is documented instead. The commands are
stable and already the ones the generated hooks ran:

- commit: `criv watch --once`, `criv check`, `criv enforce --stage commit`
- push: `criv enforce --stage push`

`docs/tooling.md` carries worked configuration for hk and for lefthook, and
`README.md` points at it.

This is a breaking change for anyone passing `--no-hooks` or `--force-hooks`, and
for vaults that relied on `criv init` wiring hooks for them. Those vaults keep
their existing `.githooks` directory and `core.hooksPath` setting; criv simply
stops managing them, and the files can be deleted or adopted by hand.

## Consequences

A repository's hook runner is chosen by the repository. criv stops competing for
a slot it does not own, and a project already using hk, lefthook, pre-commit, or
husky keeps working when criv is added to it.

criv loses the ability to configure a vault's hooks in one command. That
convenience was real, and it is deliberately traded for not silently disabling
another tool's hooks. Documentation is the replacement, which is weaker than
automation and is the honest cost of this decision.

This repository unsets `core.hooksPath` and deletes `.githooks/`, which activates
the hk hooks that ADR-0013 always intended to run.

`criv init` no longer needs to discover the git worktree, so the git subprocess
calls, the bare-repository handling, and the executable-bit logic are removed
with it.

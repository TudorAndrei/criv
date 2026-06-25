---
id: ADR-0013
kind: decision
title: Mise Managed hk Hook Toolchain
status: accepted
date: 2026-05-14
governs:
  - hk.pkl
  - mise.toml
  - README.md
---

# Mise Managed hk Hook Toolchain

## Context

criv already has stage-aware checks for commit, push, and CI enforcement, but
developers still need a repeatable local entry point for those checks. Running
the Rust commands directly leaves tool installation and hook wiring to each
machine, which makes drift more likely.

The current hook configuration lives in `hk.pkl`, while `mise.toml` pins the
hk version and installs the Git hooks through `hk install --mise`. The README
documents the user-facing setup in `README.md`.

## Decision

Use mise as the project tool installer and task front door, and use hk as the
Git hook and local check orchestrator.

`mise.toml` pins hk, sets `HK_MISE=1`, and sets `HK_PKL_BACKEND=pklr` so hk can
load `hk.pkl` without requiring a separate `pkl` CLI. Its postinstall hook runs
`hk install --mise`, causing installed Git hooks to execute through `mise x` and
therefore use the pinned tool versions.

`hk.pkl` owns the hook behavior:

- `commit-msg` runs `Builtins.check_conventional_commit`.
- `pre-commit` runs formatting, `criv check`, and commit-stage enforcement.
- `pre-push` runs clippy, workspace tests, and push-stage enforcement.
- `check` runs the full local validation set, including CI-stage enforcement.
- `fix` runs fixable formatting and documentation checks.

When hk provides a built-in step for a check, prefer that built-in over a shell
wrapper. The conventional commit hook follows this rule by using
`Builtins.check_conventional_commit` directly.

## Consequences

Local setup is a single `mise install`, and manual validation can use
`mise run check`, `mise run fix`, or the hook-specific mise tasks.

Hook behavior is centralized in hk rather than duplicated across shell scripts,
Git hook files, and release documentation. Upgrading hk requires keeping the
mise tool pin and the Pkl package URLs in `hk.pkl` aligned.

Changes to hook policy should be validated with `hk validate` and then through
the criv documentation checks so the ADR graph, README, and hook config stay in
sync.

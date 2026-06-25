---
id: ADR-0022
kind: decision
title: Hosted CI Entry Point
status: accepted
date: 2026-06-16
governs:
  - .github/workflows/*.yml
  - hk.pkl
  - mise.toml
---

# Hosted CI Entry Point

## Context

[[0013-mise-managed-hk-hook-toolchain|ADR-0013]] established `mise.toml` as
the project tool installer and task front door, with `hk.pkl` owning the
local hook and full `check` behavior.

[[0018-offline-zizmor-actions-security-check|ADR-0018]] added offline zizmor to
local checks and explicitly deferred a separate hosted GitHub Actions workflow
until criv had decided its broader CI entry point. At that time the repository
only had the tag-triggered `.github/workflows/release.yml` workflow, so adding
a hosted check for one tool would have duplicated part of local hook policy
without defining the full hosted validation boundary.

The current improvement audit found that this deferral now leaves an important
gap: contributors can rely on local hooks, but there is no normal pull-request
or main-branch CI path that proves the complete repository validation set before
release automation runs.

## Decision

Add a hosted GitHub Actions CI workflow for pull requests and pushes to `main`.

The hosted workflow should use the repository's existing validation entry point
instead of maintaining a parallel command list. In practice, CI should install
the mise-managed toolchain and run:

```sh
mise run check
```

This keeps hosted CI aligned with local validation. The `check` task remains
owned by `hk.pkl` and continues to include Cargo formatting, clippy, workspace
tests, workflow validation, offline zizmor, Obsidian plugin checks, `criv
check`, and CI-stage enforcement.

Use the same workflow security posture as `.github/workflows/release.yml`:
minimal default permissions, pinned third-party actions, and checkout with
`persist-credentials: false`.

Keep local zizmor execution offline. If a future hosted workflow wants online
zizmor or SARIF/code-scanning upload, that should be added as an explicit
extension to hosted CI, not as a replacement for the deterministic local hook
path.

## Consequences

Pull requests get the same validation contract as local `mise run check`,
closing the gap between local hooks and release-time checks.

The hosted workflow may be slower than a hand-picked set of commands, but the
single entry point avoids drift between `hk.pkl`, documentation, local hooks,
and CI.

Changes to `hk.pkl` and `mise.toml` now affect both local checks and hosted
CI. Reviewers should treat changes to those files as validation-boundary
changes and verify them with `hk validate` and the hosted workflow.

Branch pushes other than `main` are intentionally not a required hosted CI
trigger. Pull requests provide pre-merge coverage, and `main` pushes provide
post-merge protection without duplicating every developer branch run.

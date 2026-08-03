---
id: ADR-0061
kind: decision
title: Hook-Owned Local Validation And Direct CI Profile
status: accepted
date: 2026-08-03
supersedes:
  - ADR-0060
governs:
  - .github/workflows/ci.yml
  - hk.pkl
  - mise.toml
---

# Hook-Owned Local Validation And Direct CI Profile

## Context

[[0060-parallel-hosted-validation-and-lean-local-hooks|ADR-0060]] separated
repository-core validation from hosted companion lanes, but retained
`mise run check` as both a local completion gate and the hosted core entry
point. That aggregate repeats checks already owned by the automatic pre-commit
and pre-push phases. Telling agents to invoke it after commits makes the hook
phases advisory in practice and pays for the same validation twice.

Hosted CI has a different responsibility. It must validate a clean checkout
against the pull-request base even when a contributor bypassed or lacked local
hooks, so its complete profile is independent of the local development loop.

## Decision

Local development treats the installed hk hooks as the automatic validation
boundary. Pre-commit owns fast formatting, workflow, criv check, and commit
enforcement; pre-push owns Clippy, workspace tests, Hawk, and push enforcement.
Agents and contributors do not replay those phases with an aggregate finishing
command. Targeted commands remain appropriate while diagnosing a failure or
developing a companion.

The `mise run check` task is removed, along with its agent verification
instruction and normal local tooling entry point.

Hosted CI retains the four required Linux lanes, stable `Repository checks`
aggregate, direct annotation step, cached Rust builds, required companion
suites, and non-gating Windows job decided by ADR-0060. The core lane invokes
the complete hk `check` profile directly with `hk check --all`; it does not go
through a local mise task. This profile is CI orchestration, not a local
after-commit instruction.

## Consequences

Normal commits and pushes provide the intended local feedback once, at their
own lifecycle boundaries. A hook bypass can still reach the remote, but hosted
CI independently reruns the authoritative clean-checkout profile.

The hk `check` profile remains necessary for hosted CI and explicit debugging,
even though it is no longer a routine local completion gate. CI and local hooks
share step definitions without retaining a local aggregate entry point.

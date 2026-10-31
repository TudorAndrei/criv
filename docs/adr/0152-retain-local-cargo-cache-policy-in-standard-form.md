---
id: ADR-0152
kind: decision
title: Retain Local Cargo Cache Policy in Standard Form
status: accepted
date: 2026-10-30
supersedes:
  - ADR-0142
governs:
  - mise.toml
  - mise.lock
  - hk.pkl
  - .config/wt.toml
  - .worktreeinclude
---

# Retain Local Cargo Cache Policy in Standard Form

## Context

[[0142-use-mbx-for-local-cargo-caching|ADR-0142]] introduced local mbx caching
in a narrative record. The development tools have advanced, and
[[0148-separate-markdown-formatting-from-vault-validation|ADR-0148]] requires
a constrained structure for the active record.

## Decision

Keep mbx as the mise Cargo wrapper, with a pinned release and locked download
checksums. The current development release is 1.29.0; the pin is maintained in
mise rather than treated as a permanent product requirement. Require command-wrapper support from mise and keep hook commands in hk under ADR-0049.

Keep Worktrunk's existing copies of target directories and removal hook.
Disable mbx-managed targets in the wrapper so a shared target symlink cannot be
copied into another worktree. This adopts compiler-result reuse, not mbx target
placement or target cleanup.

Bypass mbx in CI and GitHub Actions. Preserve a caller's local MBX_DISABLE.
Hawk disables mbx, clears RUSTC_WRAPPER, and uses target/hawk. Keep ADR-0108's
hosted cache policy; remote caches and hosted release-build changes remain
outside this decision.

Keep the rollback path documented in [[tooling]]: remove the wrapper and
project bypass setting, then run `mise reshim`. Retain local build files; cache and tool
removal are independent. Keep issue #196 as the measurement record.

## Consequences

The active decision is formatted without altering accepted history. The tool
adds an installer dependency, shared cache, and incremental-compilation policy.
No performance gain is claimed. Managed targets and hosted cache replacement
still need separate decisions.

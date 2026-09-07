---
id: ADR-0142
kind: decision
title: Use mbx for local Cargo caching
status: accepted
date: 2026-09-07
governs:
  - mise.toml
  - mise.lock
  - hk.pkl
  - .config/wt.toml
  - .worktreeinclude
---

# Use mbx for local Cargo caching

The maintainer requested local integration of Mr. Boxington from
[issue #196](https://github.com/TudorAndrei/criv/issues/196). Use mbx 1.9.0 as
the project Cargo wrapper through mise. Pin the release and lock its download
checksums. Require mise 2026.8.16 or later for command-wrapper support. Keep
hook check commands in `hk.pkl` under
[[0049-checks-defined-in-hk-not-mise|ADR-0049]].

Keep the existing Worktrunk copies of `target/` and their removal hook. Disable
mbx-managed targets in the Cargo wrapper. A shared target symlink must not be
copied into a second worktree. This combines the existing copy behavior with
compiler-result reuse after worktrees change independently. It does not adopt
mbx target placement or target cleanup.

Bypass mbx when `CI` or `GITHUB_ACTIONS` is nonempty. Preserve the caller's
`MBX_DISABLE` setting for local commands. Hawk sets that variable to `1`, clears
`RUSTC_WRAPPER`, and uses `target/hawk`. Hosted cache policy remains governed by
[[0108-bounded-hosted-rust-compilation|ADR-0108]]. Remote caches and changes to
hosted release builds are outside this decision.

The integration adds an installer dependency, a local shared cache, and mbx's
incremental-compilation policy. It does not establish a performance gain.
Issue #196 remains the place for repeated build-time and physical disk-use
measurements against the current Worktrunk copy setup. Managed targets or a CI
cache replacement need a separate decision.

For one plain Cargo invocation, set `MBX_DISABLE=1`. For rollback, remove the
Cargo wrapper tables and the project `MBX_DISABLE` entry, then run `mise reshim`.
Keep existing local build files;
the shared cache and tool pin can be removed separately. See [[tooling]] for
setup, inspection, and rollback commands.

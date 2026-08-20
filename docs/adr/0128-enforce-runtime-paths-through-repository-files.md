---
id: ADR-0128
kind: decision
title: Enforce Runtime Paths Through Repository Files
status: accepted
date: 2026-08-21
supersedes:
  - ADR-0125
governs:
  - src/adr.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
  - src/c4.rs
  - src/c4/**/*.rs
  - src/check.rs
  - src/discovery/**/*.rs
  - src/enforce.rs
  - src/init.rs
  - src/init/templates.rs
  - src/install.rs
  - src/install/**/*.rs
  - src/lib.rs
  - src/policy_scan.rs
  - src/query.rs
  - src/repository.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/state.rs
  - src/state/**/*.rs
  - src/structural.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-wasm/src/**/*.rs
  - assets/likec4-bridge.mjs
  - extensions/vscode-criv/src/diagnostics/model.ts
  - extensions/vscode-criv/src/diagnostics/publisher.ts
  - .github/workflows/release.yml
  - README.md
policy:
  patterns:
    - id: no-caller-owned-reconciliation-snapshot
      language: rust
      rule: |
        any:
          - pattern: struct TransactionSnapshot { $$$FIELDS }
          - pattern: struct PathSnapshot { $$$FIELDS }
      message: Reconciliation callers must use the shared Snapshot owner in src/adr/reconcile_transaction.rs.
    - id: no-private-adr-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::adr::(source_reconcile|reconcile_transaction)(::|$)'
      message: Callers must use the src/adr.rs interface, not a private reconciliation child.
    - id: no-private-c4-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::c4::(artifact|likec4)(::|$)'
      message: Callers must use the src/c4.rs interface, not a private C4 child.
    - id: no-private-install-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::install::(editor|skills)(::|$)'
      message: Callers must use the src/install.rs interface, not a private installation child.
    - id: no-private-repository-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::repository::filesystem(::|$)'
      message: Callers must use the src/repository.rs interface, not its private filesystem child.
    - id: confined-repository-mutations-only
      language: rust
      rule: |
        all:
          - any:
              - pattern: std::fs::write($$$ARGS)
              - pattern: fs::write($$$ARGS)
              - pattern: std::fs::rename($$$ARGS)
              - pattern: fs::rename($$$ARGS)
              - pattern: std::fs::remove_file($$$ARGS)
              - pattern: fs::remove_file($$$ARGS)
              - pattern: std::fs::remove_dir_all($$$ARGS)
              - pattern: fs::remove_dir_all($$$ARGS)
              - pattern: std::fs::create_dir_all($$$ARGS)
              - pattern: fs::create_dir_all($$$ARGS)
              - pattern: File::create($$$ARGS)
              - pattern: OpenOptions::new()
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: Repository mutations must use the RepositoryFiles interface in src/repository.rs; direct filesystem mutation is test-only.
    - id: no-native-linter-subprocess
      language: rust
      rule: |
        any:
          - pattern: Command::new("ruff")
          - pattern: Command::new("oxlint")
          - pattern: Command::new("eslint")
      message: ADR-0046 keeps native language linters outside criv enforce and the runtime.
    - id: no-global-likec4-subprocess
      language: rust
      rule: |
        any:
          - pattern: Command::new("npx")
          - pattern: Command::new("likec4")
      message: ADR-0074 requires the locked local Node bridge, not npx or a global LikeC4 command.
    - id: no-removed-command-modules
      language: rust
      rule: |
        any:
          - pattern: mod search;
          - pattern: mod measurement;
      message: ADR-0072 and ADR-0082 removed core measurement and the standalone search command.
    - id: no-obsolete-init-flags
      language: rust
      rule: |
        any:
          - pattern: '"no-obsidian"'
          - pattern: '"no-vscode"'
          - pattern: '"no-hooks"'
          - pattern: '"force-hooks"'
      message: ADR-0054 and ADR-0128 keep hook and editor actions out of criv init.
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
---

# Enforce Runtime Paths Through Repository Files

## Context

[[0125-own-installation-implementations-under-the-install-module|ADR-0125]]
owns the current runtime module tree and its active policies. Its repository
mutation policy names confined helpers in `src/util.rs`.

[[0127-own-repository-files-behind-one-interface|ADR-0127]] moves repository
file access to the `src/repository.rs` parent interface and its private
`src/repository/filesystem.rs` implementation. Accepted decisions do not
change. A new decision must update the policy path and keep every other active
runtime rule.

## Decision

Retain the complete module ownership, installation behavior, C4 behavior,
diagnostic location contract, reconciliation contract, Git boundary, command
surface, State formats, and editor behavior from ADR-0125.

Use `src/repository.rs` as the only caller interface for repository file
access. Callers must not import `crate::repository::filesystem`. Keep the
private filesystem child outside this decision's `governs` paths because that
child owns the raw operating-system mutations that the runtime policy forbids
for callers.

Retain every policy from ADR-0125. Change only the confined mutation message
so it names the Repository Files interface. Add the private Repository Files
child-import policy. Apply the policy to `src/repository.rs`, which does not
contain raw mutations, and do not apply it to
`src/repository/filesystem.rs`.

Keep `src/install.rs` as the only installation interface, with private editor
and generated-skill children. Keep Init and editor installation as separate
commands. Keep `src/c4.rs` as the only C4 interface. Keep `src/adr.rs` as the
only reconciliation interface. Keep the shared reconciliation snapshot in its
private ADR child. Keep `git2` behind `src/git.rs`.

Keep native linters outside the runtime. Keep LikeC4 execution on the locked
local bridge. Keep removed command modules and obsolete Init flags absent.
Keep all current CLI text, receipt schemas, transaction order, rollback
behavior, path rules, permissions, State schemas, and serialized output.

## Consequences

Runtime policy text now matches the implemented Repository Files owner. The
raw filesystem implementation can perform its required private work without
making raw mutations legal in callers.

ADR-0125 becomes historical only because its repository mutation policy names
the old helper path. All other behavior and policy from ADR-0125 stays active
through this decision.

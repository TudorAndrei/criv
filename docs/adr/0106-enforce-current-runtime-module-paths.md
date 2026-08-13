---
id: ADR-0106
kind: decision
title: Enforce Current Runtime Module Paths
status: accepted
date: 2026-08-13
supersedes:
  - ADR-0090
governs:
  - src/adr.rs
  - src/architecture.rs
  - src/check.rs
  - src/enforce.rs
  - src/init.rs
  - src/init/templates.rs
  - src/install_editor.rs
  - src/lib.rs
  - src/likec4.rs
  - src/query.rs
  - src/source.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/source_reconcile.rs
  - src/state.rs
  - src/state/**/*.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-wasm/src/**/*.rs
policy:
  patterns:
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
      message: Repository mutations must use the confined helpers in src/util.rs; direct filesystem mutation is test-only.
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
      message: ADR-0054 and ADR-0087 removed hook and editor actions from criv init.
    - id: fff-only-through-source-index
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^fff_search::'
      message: ADR-0006 and ADR-0042 put fff-search behind src/source/index.rs.
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
---

# Enforce Current Runtime Module Paths

## Context

[[0090-enforce-runtime-boundary-decisions|ADR-0090]] added structural checks for
runtime boundaries. [[0105-owner-scoped-rust-module-layout|ADR-0105]] moves
Source, State publication, snapshots, and Wasm implementation into owner
directories. ADR-0090 names old source paths and gives the `fff-search` owner
as `src/source_index.rs`.

The ownership rules did not change. Their governed paths and messages must
match the current module tree.

## Decision

Keep the runtime rules from ADR-0090 and apply them to the current owner paths.
Repository mutation stays behind the confined helpers. Native linters stay out
of the runtime. LikeC4 execution stays on the locked local bridge. Removed
commands and initialization flags stay removed.

Keep `fff-search` behind `src/source/index.rs`. Keep `git2` behind `src/git.rs`.
The new child paths are part of the governed production scope. The Source index
stays outside the policy scope because it is the one owner of `fff-search`.
Direct file mutation remains valid only in inline Rust test modules.

## Consequences

The structural rules scan nested Rust modules and no implementation escapes a
rule because it moved into an owner directory. Policy diagnostics name current
paths.

## Alternatives Considered

### Keep ADR-0090 active with old paths

Rejected. Its `governs:` entries and policy message would not describe the
current module tree.

### Keep forwarding files at old paths

Rejected. Forwarding files would add shallow interfaces only to satisfy old
path text.

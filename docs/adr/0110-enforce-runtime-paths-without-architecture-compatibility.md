---
id: ADR-0110
kind: decision
title: Enforce runtime paths without architecture compatibility
status: accepted
date: 2026-08-13
supersedes:
  - ADR-0106
governs:
  - src/adr.rs
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

# Enforce Runtime Paths Without Architecture Compatibility

## Context

[[0106-enforce-current-runtime-module-paths|ADR-0106]] includes the obsolete
`src/architecture.rs` compatibility module in its active scope. ADR-0109
removes that module.

## Decision

Keep the runtime boundary rules and current paths from ADR-0106, except for the
removed architecture compatibility module. The policy definitions above are
their current owners.

## Consequences

Runtime policy checks keep their owner exceptions and do not refer to a deleted
source path.

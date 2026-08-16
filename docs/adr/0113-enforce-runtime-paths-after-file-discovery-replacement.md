---
id: ADR-0113
kind: decision
title: Enforce runtime paths after file discovery replacement
status: accepted
date: 2026-08-16
supersedes:
  - ADR-0110
governs:
  - src/adr.rs
  - src/check.rs
  - src/discovery/**/*.rs
  - src/enforce.rs
  - src/init.rs
  - src/init/templates.rs
  - src/install_editor.rs
  - src/lib.rs
  - src/likec4.rs
  - src/query.rs
  - src/source.rs
  - src/source/catalog.rs
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
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
---

# Enforce Runtime Paths After File Discovery Replacement

## Context

[[0110-enforce-runtime-paths-without-architecture-compatibility|ADR-0110]] keeps
an ownership rule for `fff-search` and names `src/source/index.rs` as its only
owner. [[0112-direct-ignore-file-discovery|ADR-0112]] removes that dependency
and module. It adds `src/discovery/` and `src/source/catalog.rs` to production
runtime scope.

## Decision

Keep every runtime boundary rule from ADR-0110 except
`fff-only-through-source-index`. There is no `fff-search` runtime owner after
ADR-0112.

Apply the remaining rules to the current module paths. Repository mutation
stays behind confined helpers. Native linters stay out of the runtime. LikeC4
execution stays on the locked local bridge. Removed commands and initialization
flags stay removed. Direct `git2` use stays in `src/git.rs`.

## Consequences

Runtime policy scans cover the discovery and Source catalog modules without
referring to a deleted dependency or file. All other runtime restrictions keep
the same policy IDs and behavior.

## Alternatives Considered

### Keep an empty fff ownership rule

Rejected. A policy for a removed dependency gives false architecture guidance.

### Rewrite every runtime policy

Rejected. The file-discovery replacement changes only the obsolete `fff-search`
owner rule and governed module paths.

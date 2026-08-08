---
id: ADR-0090
kind: decision
title: Enforce Runtime Boundary Decisions
status: accepted
date: 2026-08-08
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
  - src/snapshots.rs
  - src/source_graph.rs
  - src/source_reconcile.rs
  - src/state.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-wasm/src/lib.rs
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
      message: ADR-0006 and ADR-0042 put fff-search behind src/source_index.rs.
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
---

# Enforce Runtime Boundary Decisions

## Context

The accepted-ADR compliance audit found that most runtime decisions were
covered by tests or review but had no structural guard. It also found two
direct `fs::remove_file` calls in the watch-lock lifecycle. Those calls bypassed
the confinement boundary selected by
[[0044-vault-write-confinement|ADR-0044]].

The same audit confirmed that removed command modules, native linter
subprocesses, global LikeC4 commands, and direct `fff-search` or `git2` use were
absent. Without a policy, a later refactor can reintroduce them while normal
tests remain green.

Ast-grep can enforce these syntax boundaries. It cannot prove runtime ordering,
filesystem atomicity, or complete command behavior. Those properties remain
test responsibilities.

## Decision

Add accepted inline policies for structural runtime boundaries.

Repository mutations in governed production modules use confined helpers from
`src/util.rs`. Direct filesystem mutation remains valid inside inline Rust test
modules, where tests create and remove isolated fixtures. The confinement
helper implementation is outside this policy scope because it is the one owner
of the raw filesystem operations.

Keep native linter subprocesses out of criv. Keep LikeC4 execution on the
locked local Node bridge. Do not restore the removed search or core measurement
modules. Do not restore removed hook or editor flags on `criv init`.

Keep `fff-search` behind `src/source_index.rs` and `git2` behind `src/git.rs`.
The governed modules consume criv-owned source and Git values instead of
dependency objects.

Each policy is a prohibited syntax pattern. A match is an enforcement error;
an absent match is not proof of complete behavioral compliance.

## Consequences

`criv check` and `criv enforce` now reject common ways to bypass effective
runtime decisions before the bypass becomes a new behavior path.

The policies deliberately do not ban all process execution or filesystem I/O.
The Node bridge, explicit editor installation, process liveness probes, reads,
and the confined helper implementation remain valid. Tests continue to verify
the behavior that structural matching cannot prove.

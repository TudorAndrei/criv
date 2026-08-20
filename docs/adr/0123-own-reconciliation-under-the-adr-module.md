---
id: ADR-0123
kind: decision
title: Own reconciliation under the ADR module
status: accepted
date: 2026-08-20
supersedes:
  - ADR-0101
  - ADR-0113
governs:
  - src/adr.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
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
  - src/state.rs
  - src/state/**/*.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-wasm/src/**/*.rs
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

# Own Reconciliation Under the ADR Module

## Context

[[0101-one-reconciliation-transaction-owner|ADR-0101]] gives ADR identity
reconciliation and governed source reconciliation one shared rollback owner.
It keeps the two reconciliation meanings separate. The implementation modules
still appear as three root peers in `src/lib.rs`, although the `adr` command is
the only command owner of both reconciliation operations.

`src/enforce.rs` also reads governed source reconciliation receipts. It calls
the source reconciliation implementation directly instead of using the ADR
interface. This exposes a private implementation seam outside its owner.

[[0105-owner-scoped-rust-module-layout|ADR-0105]] requires the file tree and
Rust module tree to show the same owner. It assigns source reconciliation to
Governance because it changes ADR source references. The path replacement and
complete Git index rollback corrections are complete, so the safety-critical
implementation can now move without mixing the move with those corrections.

[[0113-enforce-runtime-paths-after-file-discovery-replacement|ADR-0113]] keeps
the active runtime policies but names the old root source reconciliation path.
The accepted ADR is immutable. A new effective decision must carry those
policies onto the current module tree.

## Decision

Make `src/adr.rs` the owner interface for all ADR reconciliation behavior. Move
governed source reconciliation to `src/adr/source_reconcile.rs`. Move the
shared rollback owner to `src/adr/reconcile_transaction.rs`. Declare both as
private child modules from `src/adr.rs`. Remove their root module declarations
from `src/lib.rs`. Do not keep forwarding modules at the old paths.

Keep the two reconciliation meanings separate. The ADR interface continues to
own provisional identifier planning, proof, receipts, commit text, and error
text. The source reconciliation child continues to own exact source rename
planning, governed-scope rewrites, its receipt, its commit text, and its error
text. The transaction child continues to capture and restore the complete Git
index, file contents, permissions, and absent paths for both operations.

Expose only source-reconciliation receipt and history questions that staged
enforcement needs through named items on the ADR interface. Enforcement must
not import the private child. Command routing also uses only the ADR interface.
The child modules remain internal seams whose tests can directly prove their
safety and parsing behavior.

Keep all runtime policies from ADR-0113. Keep the shared snapshot policy from
ADR-0101. Apply them to the new ADR child paths and the existing runtime scope.
No command, receipt schema, transaction order, rollback behavior, State text,
or user-facing output changes.

Record `criv::adr::source_reconcile` and
`criv::adr::reconcile_transaction` as private Governance implementations in
the Code architecture map. Keep `criv::adr` as their owning module identity.
Remove the direct Code relationship from enforcement to the source
reconciliation child because enforcement now depends on the ADR interface.

## Consequences

The file tree and Rust module tree now show one Governance owner. Callers learn
one ADR interface. Source reconciliation and transaction recovery keep their
deep implementations and focused internal tests without becoming independent
root interfaces.

Changes to reconciliation behavior, receipt proof, or rollback stay local to
the ADR owner scope. Runtime policy scans continue to cover the same production
code with current paths. Existing CLI and enforcement tests prove that the move
does not change behavior.

## Alternatives Considered

### Keep the root peer modules

Rejected. Root peers make private Governance implementations look like
independent interfaces and let callers bypass the ADR seam.

### Keep forwarding modules at the old paths

Rejected. Forwarding modules add shallow interfaces only to preserve internal
path text.

### Combine both reconciliation implementations

Rejected. Their planning, receipt, proof, commit, and error contracts are
different. Only rollback state is shared.

### Move source reconciliation under Source intelligence

Rejected. The implementation changes ADR `governs:` references. It does not
discover, parse, or index source code.

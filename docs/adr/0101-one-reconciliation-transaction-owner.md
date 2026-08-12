---
id: ADR-0101
kind: decision
title: One Reconciliation Transaction Owner
status: accepted
date: 2026-08-12
governs:
  - src/adr.rs
  - src/source_reconcile.rs
  - src/reconcile_transaction.rs
  - src/lib.rs
policy:
  patterns:
    - id: no-caller-owned-reconciliation-snapshot
      language: rust
      rule: |
        any:
          - pattern: struct TransactionSnapshot { $$$FIELDS }
          - pattern: struct PathSnapshot { $$$FIELDS }
      message: Reconciliation callers must use the shared Snapshot owner in src/reconcile_transaction.rs.
---

# One Reconciliation Transaction Owner

## Context

[[0063-adr-reconciliation-owns-its-commit|ADR-0063]] requires ADR identity
reconciliation to restore touched worktree paths, the Git index, and its prior
receipt after every reported failure before commit.
[[0073-effective-adr-governance-and-source-reconciliation|ADR-0073]] requires
the same recovery for governed source reconciliation.

`src/adr.rs` and `src/source_reconcile.rs` each implemented a transaction
snapshot and a path snapshot. Both implementations captured the Git index,
file contents, file permissions, and absent paths. Both restored the index and
every captured path after a failure. They differed only in receipt path,
receipt data, and command-specific error text.

This duplicated safety-critical file recovery. A confinement or recovery fix
could reach one command and not the other. Combining the complete commands
would create a broad module because their planning, receipt, proof, and error
contracts are different.

## Decision

Make `src/reconcile_transaction.rs` the only private owner of reconciliation
snapshot capture and rollback. Its interface has two operations:

- `Snapshot::capture(root, paths)` captures the Git index and every named path.
- `Snapshot::rollback(root)` tries to restore the Git index and every named
  path, then returns all rollback error messages.

The caller passes every path that the transaction can change. This list must
include its receipt path. The shared module captures regular UTF-8 file
contents, permissions, and file absence. It restores files through the
confined atomic write helpers, removes files that were absent at capture time,
and continues after an individual restore failure so later paths still get a
recovery attempt.

Keep reconciliation meaning in each caller. `src/adr.rs` owns provisional ADR
planning, its receipt schema and path, staged proof, commit message, and
ADR-specific rollback error text. `src/source_reconcile.rs` owns source rename
planning, its separate receipt schema and path, staged proof, commit message,
and source-specific rollback error text. Neither receipt type becomes part of
the shared interface.

Migrate ADR identity reconciliation first and run its existing validation
rollback test. Then migrate governed source reconciliation and run its existing
validation rollback test. Remove both caller-local snapshot implementations
after both paths use the shared module.

Test the shared interface directly. Prove that rollback restores the Git index,
existing file contents and permissions, and an absent receipt path. Also prove
that one failed path restore does not stop a later path restore. Keep both
command-level rollback tests to prove caller integration and error behavior.

Enforce the single owner with
[[match:ADR-0101/no-caller-owned-reconciliation-snapshot]]. The rule rejects the
old `TransactionSnapshot` and `PathSnapshot` declarations in all governed
Rust files.

This decision refines the private implementation of ADR-0063 and ADR-0073. It
does not supersede them or change either command contract.

## Consequences

Index and file recovery now have one owner and one direct test surface. A
future confinement or rollback correction applies to both reconciliation
commands.

Callers must build a complete path list before capture. A missing path cannot
be restored by the shared module. Receipt policy and user-facing messages stay
separate, so the shared interface remains narrow.

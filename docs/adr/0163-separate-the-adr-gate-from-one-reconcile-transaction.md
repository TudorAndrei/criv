---
id: ADR-0163
kind: decision
title: Separate the ADR Gate from One Reconcile Transaction
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0150
governs:
  - src/adr.rs
  - src/adr/gate.rs
  - src/adr/receipt.rs
  - src/adr/governs.rs
  - src/adr/identity_reconcile.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
  - src/enforce.rs
  - src/lib.rs
policy:
  patterns:
    - id: no-private-adr-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::adr::(gate|receipt|governs|identity_reconcile|source_reconcile|reconcile_transaction)(::|$)'
      message: Callers must use the src/adr.rs interface, not a private ADR child.
    - id: confined-repository-access-only
      language: rust
      rule: |
        all:
          - any:
              - pattern: std::fs::write($$$ARGS)
              - pattern: fs::write($$$ARGS)
              - pattern: std::fs::read_to_string($$$ARGS)
              - pattern: fs::read_to_string($$$ARGS)
              - pattern: std::fs::read($$$ARGS)
              - pattern: fs::read($$$ARGS)
              - pattern: fs::symlink_metadata($$$ARGS)
              - pattern: fs::remove_file($$$ARGS)
              - pattern: File::open($$$ARGS)
              - pattern: OpenOptions::new()
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: ADR code reads and writes repository files through RepositoryFiles; ambient file access is test-only.
---

# Separate the ADR Gate from One Reconcile Transaction

## Context

[[0150-retain-complete-adr-change-ownership-in-standard-form|ADR-0150]] gives
the ADR module the complete change decision that enforcement calls, and keeps
the source receipt proof in the source reconciliation child.
[[0123-own-reconciliation-under-the-adr-module|ADR-0123]] gives snapshot
capture and rollback to the transaction child.

`src/adr.rs` held the gate and the identity reconcile command in one file, so
the gate shared the command's dependency on `check`. The two reconcile
commands each wrote the same sequence: repository check, target resolution,
plan, `--check` exit, dirty check, identity preflight, three stable-target
checks, snapshot, write, validation, receipt, staging, proof, commit, and
rollback error merge. Each had its own hash function. The identity receipt
had three copies of its before/after file check, and each proof re-read the
receipt and re-checked its schema. A failed Git read counted as an absent
file, so a read error could prove a deletion.

## Decision

`src/adr.rs` is the interface: the `adr` command options, `run`,
`change_violations`, and `reconcile_identities` with its `Mode`. CI calls
`reconcile_identities` in `Mode::Check` before enforcement.

- `src/adr/gate.rs` is the gate: immutability, the portable-link migration
  exception, the branch-local CI exception, and the scope-move check. It opens
  the repository and reads each receipt once. It does not depend on `check`.
- `src/adr/receipt.rs` owns both receipt types, their schemas and paths, one
  `hash`, and every receipt proof. A receipt file proof compares a recorded
  hash and mode with a `git::Revision`: a commit, the index, or the worktree.
  A missing file proves absence; a failed read proves nothing. Source history
  proof moves here from the source child, which supersedes that part of
  ADR-0150.
- `src/adr/governs.rs` owns the exact `governs:` rewrite that the source
  command writes and the source proofs check.
- `src/adr/reconcile_transaction.rs` owns the transaction skeleton, the
  `Reconciliation` trait that the two commands implement, the one stable-target
  guard, the after-write validation, the snapshot, and the Git index snapshot
  with its `index.lock` restore.
- `src/adr/identity_reconcile.rs` and `src/adr/source_reconcile.rs` own only
  their plans, writes, and receipts.

A run opens one `RepositoryFiles` value and one Git repository. ADR code has
no ambient file access, as [[match:ADR-0163/confined-repository-access-only]]
requires. Callers use only the interface, as
[[match:ADR-0163/no-private-adr-child-import]] requires for every child.

The receipt schemas, their paths, the enforcement order, and the failure text
of ADR-0150 do not change.

## Consequences

A change to the transaction order, a receipt proof, or the hash has one place.
The gate can change without the reconcile commands, and the reverse.

Both commands now report a moved target as "target ref `X` moved before,
during, or before the commit of" the named reconciliation, and a validation
failure names the reconciliation.

A receipt file that Git cannot read no longer proves an absent input or a
deleted output, and a recorded hash without its mode proves nothing.

A rewrite edit outside the text is an error instead of a silent skip.

---
id: ADR-0057
kind: decision
title: Branch-Local ADR Publication And Reconciliation
status: accepted
date: 2026-08-02
supersedes:
  - ADR-0012
governs:
  - src/adr.rs
  - src/git.rs
  - src/enforce.rs
  - src/util.rs
  - README.md
---

# Branch-Local ADR Publication And Reconciliation

## Context

[[0012-adr-immutability-enforcement|ADR-0012]] treated every committed ADR as
published. Two independent branches can therefore legitimately create the same
next numeric ID, yet the branch merged second cannot change its ADR filename or
references without violating the immutable-ADR rule. Reserving numbers through
a hosted service would couple a local CLI to a particular forge and still leave
allocation races during integration.

## Decision

An ADR becomes published when it is present in the exact target commit chosen
for integration, rather than merely when it is committed on a branch. ADRs
already in that target remain immutable. ADRs absent from it are provisional
branch-local records and may be reconciled by `criv adr reconcile --base <ref>`.

The command resolves the base once to an exact SHA, finds the merge base, and
allocates all branch-local ADRs as one contiguous block after the target's
largest numeric ID when their IDs or paths conflict with the target. It orders
that block by original numeric ID. Its check mode is read-only and reports the
SHA, mapping, and repair command; write mode performs the deterministic
renames and simultaneous reference replacements.

Only branch-owned content may change. A new branch file may be rewritten in
full; in a file that existed at the merge base, only branch-added diff lines
are eligible. Ambiguous ownership, malformed identities, incomplete history,
binary or non-UTF-8 content, a changed target ref, and destination collisions
must fail closed. The command records its exact input and output hashes in an
ignored `.criv/adr-reconcile.json` receipt. Local enforcement admits an ADR
rename only when staged blobs match that receipt; absent or stale proof keeps
the conservative immutability rejection.

Integration remains externally serialized: a coordinator must read the target
SHA, reconcile, validate and commit, then compare that SHA again before merge.
If it moved, the branch retries against the new target. criv neither reserves
IDs nor pushes or merges changes.

## Consequences

The reconciliation and Git modules own target-aware allocation and Git
evidence, while `src/enforce.rs` preserves append-only protection for published
ADRs. `src/util.rs` must confine and atomically publish every rewrite. The
checkout-local `criv query next-adr-id` remains a convenience query, not an
allocation authority.

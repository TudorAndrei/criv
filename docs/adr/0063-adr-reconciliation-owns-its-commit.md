---
id: ADR-0063
kind: decision
title: ADR Reconciliation Owns Its Commit
status: accepted
date: 2026-08-03
supersedes:
  - ADR-0057
governs:
  - src/adr.rs
  - src/git.rs
  - src/enforce.rs
---

# ADR Reconciliation Owns Its Commit

## Context

[[0057-branch-local-adr-publication-and-reconciliation|ADR-0057]] made
`criv adr reconcile --base <ref>` responsible for a deterministic, receipt-
proven worktree rewrite but left staging and committing to its caller. That
split allowed commit enforcement to validate the generated transaction while
push enforcement later saw a broader comparison whose new ref was a subsequent
merge commit. The exact reconciliation commit was no longer the entry's new
ref, so the valid rename could be rejected and require `--no-verify`.

The reconciliation command already requires a clean worktree, resolves the
target once, confines every write, validates the resulting vault, and records
the exact before/after blobs and modes. Asking a caller or agent to recreate the
last transactional step adds no independent safety boundary. It exposes receipt
timing and hook behavior instead.

[[0058-embedded-git-repository-access|ADR-0058]] also makes `src/git.rs` the
production repository boundary. The command must not regain a Git executable
dependency merely to create its commit.

## Decision

Write-mode ADR reconciliation owns one dedicated commit containing exactly the
receipt-proven paths. After planning, it preflights repository author identity,
applies the simultaneous rewrite, validates the complete vault, writes the
ignored receipt, stages only the receipt paths, proves the staged tree, and
creates the commit through the embedded Git boundary. Its fixed message is
`docs(adr): reconcile provisional identifiers`.

The commit uses the repository's configured `user.name` and `user.email` for
both author and committer at the time of reconciliation. Missing identity fails
before worktree mutation. The embedded commit is deliberately unsigned and does
not interpret `commit.gpgSign`; signing a generated transaction would require a
separate signer protocol and key-policy decision.

The embedded commit does not execute external Git hooks. This is a narrow
transactional bypass, not a general hook bypass: the command starts from a clean
index and worktree, validates the complete vault itself, stages only paths
listed by the receipt, and proves the exact staged tree before moving `HEAD`.
Unrelated dirty paths, partial staging, extra ADR changes, and forged or stale
receipts continue to fail closed. The fixed message also satisfies the
repository's conventional-commit contract without delegating it to a hook.

Every reported failure before the commit restores the touched worktree paths,
index entries, and any prior ignored receipt to their starting state. A process
crash may leave a materialized receipt, which remains a recoverable transaction:
rerunning reconciliation must either prove and finish that exact work or reject
it without widening the changed-path set.

Push and CI enforcement may recognize the receipt-backed commit anywhere in the
compared first-parent history, not only when it is the comparison's final new
ref. The allowance applies only when that commit is an exact child of the
receipt's planning `HEAD`, its complete diff matches the receipt, the final tree
still contains the receipt's output blobs and modes, and the individual changed
entry touches only receipt paths. A later mutation of a reconciled ADR therefore
invalidates the allowance, while unrelated later commits or a merge from the
still-pinned target do not.

The coordinator still compares the resolved target SHA immediately before
integration and reruns reconciliation if it moved. criv creates no merge,
reservation, push, or transport operation.

## Consequences

The successful command leaves a clean worktree at a dedicated, auditable commit
instead of requiring callers to understand receipt-aware hook timing. The
receipt remains local evidence for commit, push, and CI enforcement and is not
part of the committed tree.

Repositories that require signed commits cannot treat this generated commit as
signed; adding signing support requires a follow-up decision defining signer
discovery, interaction, failure recovery, and cross-platform behavior.

`src/git.rs` gains only local index, commit, identity, and history operations.
ADR allocation remains target-relative and externally serialized as in
ADR-0057; this decision changes ownership of the proven commit and replaces
ADR-0057's instruction that the coordinator create it.

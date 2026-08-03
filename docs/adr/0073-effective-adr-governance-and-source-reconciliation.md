---
id: ADR-0073
kind: decision
title: Effective ADR Governance And Source Reconciliation
status: accepted
date: 2026-08-03
supersedes:
  - ADR-0012
  - ADR-0059
governs:
  - src/**
  - README.md
---

# Effective ADR Governance And Source Reconciliation

## Context

[[0002-docs-and-adrs-form-the-governance-graph|ADR-0002]] makes unresolved
`governs:` scopes validation errors, but State refresh currently reports those
errors and publishes anyway. Removing an exactly governed source file can
therefore leave a policy with an empty scope while `.criv/state.json` appears
current.

[[0012-adr-immutability-enforcement|ADR-0012]] correctly keeps accepted ADRs
append-only, but gives a semantics-preserving source rename no mechanical
repair path. Editing the old path is rejected even when Git proves that the
source identity merely moved. Deletion is different: removing governed code
can retire or change a decision and must not be disguised as a path rewrite.

[[0059-accepted-only-adr-policy-state|ADR-0059]] activates every accepted ADR
policy without considering forward-only `supersedes:` metadata. A successor
can record deletion of the old implementation, yet the superseded policy and
its now-missing scope remain active. This currently forces historical anchor
files such as `src/measurement.rs` to remain in the source tree after
[[0072-keep-performance-observation-outside-core|ADR-0072]] superseded the
decision that introduced them.

ADR identity reconciliation already supplies the relevant safety model.
[[0057-branch-local-adr-publication-and-reconciliation|ADR-0057]] plans against
a pinned integration target and fails closed on ambiguous ownership.
[[0063-adr-reconciliation-owns-its-commit|ADR-0063]] makes the generated
transaction validate, roll back, prove, and commit its exact change.

## Decision

Define an **effective ADR** as an accepted decision that is not superseded,
directly or transitively, by another accepted decision. A draft, proposed, or
otherwise non-accepted successor does not deactivate an accepted predecessor.
Derive this status from forward `supersedes:` edges; old ADRs do not need a
mutable `superseded_by` backlink.

Only effective ADRs publish and scan active policy patterns. Historical policy
IDs remain resolvable for links, queries, and audit. This retains ADR-0059's
accepted-only boundary while preventing a superseded accepted policy from
continuing to govern current code.

Every explicit or default `governs:` entry on an effective ADR must resolve to
at least one current source file. `criv check` continues to report
`unresolved-governs` as an error. State refresh additionally treats this
condition as publication-blocking: do not publish generated architecture,
`.criv/state.json`, `.criv/latest`, or a snapshot from that candidate. Keep the
last successful in-memory refresh result so a long-running watcher can recover
after a later docs or source change. Initial refresh and `watch --once` fail.
Other validation errors retain their existing publication behavior.

Do not add a third serialized diagnostic severity. Publication blocking is a
typed internal property of active-governance validation; text, JSON, GitHub
annotations, and editor consumers retain the existing `error` and `warning`
values. The source-graph cache may reflect the observed filesystem because it
is rebuild input, not published valid State.

When a governed source path is deleted, require a new accepted ADR that
explains the deletion and lists the old decision in `supersedes:`. The
successor declares any surviving or replacement scope and policy. Once
accepted, the predecessor is historical, its missing source scope no longer
blocks publication, and its policies are no longer active. criv never authors
that decision or infers that deletion means retirement.

When Git proves a one-to-one source rename, allow
`criv adr reconcile-sources --base <ref>` to repair exact source-path scalars
under ADR `governs:`. Check mode reports the pinned target, mappings, and
required repair without writing. Write mode requires a clean worktree, rejects
deletions, copies, ambiguity, unsupported YAML, and unresolved destinations,
then performs confined simultaneous rewrites. It validates the complete vault,
writes an ignored `.criv/source-reconcile.json` receipt, stages only the proven
ADR paths, and creates one embedded-Git commit with the fixed message
`docs(adr): reconcile renamed source scopes`. Every reported failure restores
the worktree, index, and prior receipt.

This source reconciliation is the sole exception to accepted-ADR content
immutability. Commit and push/CI enforcement do not trust the command name,
commit message, or checkout-local receipt. They independently require the
compared Git range to contain the corresponding source rename and require the
accepted ADR diff to consist only of the exact mapped `governs:` scalar
substitutions. Every other modification, deletion, or rename of a published
ADR remains forbidden.

Do not infer directory mappings, rewrite broad globs, change policy bodies,
rewrite arbitrary prose or other source-reference forms, create ADRs, push, or
merge. Those operations contain intent that a source rename alone cannot
prove.

## Consequences

Published State cannot claim current governance after an active scope
disappears. The long-running watcher keeps serving its last valid result and
can recover, while one-shot and initial refreshes make the invalid repository
state visible through a failing exit status.

Accepted superseded ADRs become historical policy records rather than active
policy owners. Deleting governed behavior requires an explicit successor ADR,
so policy retirement remains append-only and reviewable without permanent
source tombstones.

Exact source renames gain a narrow, auditable repair path. The command is more
conservative than a general reference rewriter: noncanonical YAML and changes
that could alter meaning require human resolution. Independent range
verification keeps the immutability exception valid in a fresh CI clone where
the local receipt does not exist.

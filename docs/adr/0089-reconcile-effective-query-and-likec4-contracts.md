---
id: ADR-0089
kind: decision
title: Reconcile Effective Query And LikeC4 Contracts
status: accepted
date: 2026-08-08
supersedes:
  - ADR-0066
  - ADR-0075
  - ADR-0076
governs:
  - src/query.rs
  - src/lib.rs
  - packages/criv-likec4/src/protocol.ts
  - packages/criv-likec4/src/renderer.ts
  - extensions/vscode-criv/src/c4Preview.ts
  - extensions/vscode-criv/src/c4PreviewModel.ts
  - .obsidian/plugins/criv/src/main.ts
---

# Reconcile Effective Query And LikeC4 Contracts

## Context

The accepted-ADR compliance audit found three conflicts between decisions that
criv still treated as effective.

[[0066-capability-directed-wikilink-and-query-resolution|ADR-0066]] named
`c4-relationships` as a docs-only query. The LikeC4 hard cutover in
[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] removed that
Mermaid-era query, but it did not supersede ADR-0066.

[[0075-likec4-preview-as-the-default-c4-editor|ADR-0075]] selected a fallback
view when a file owned no view. [[0080-co-locate-primary-likec4-views-with-their-models|ADR-0080]]
requires an explicit empty state and no fallback, but it did not supersede
ADR-0075.

[[0076-focused-likec4-workspace-navigation|ADR-0076]] required model and view
files to live in separate folders. ADR-0080 permits a domain file to own its
model declarations and primary views. Both layout rules remained effective.

The implementation follows the newer LikeC4 and typed-query contracts. This
decision repairs the governance graph without changing the current product.

## Decision

Keep capability-directed query loading and Wikilink dispatch from ADR-0066.
Typed `source:` and `match:` targets resolve before note and legacy-source
compatibility. One vault load resolves each unique Wikilink target once.
Snapshot-only queries do not load a vault. Docs-only queries do not build a
source index or source graph. Source-requiring queries use the complete source
catalog lifecycle from [[0042-shared-source-index-lifecycle|ADR-0042]].

The docs-only query set is `next-adr-id`, `cited-by`, `orphan-docs`, and
`nodes` when it is restricted to documentation or decisions. The removed
`c4-relationships` operation is not part of the command tree. `c4-code` and all
queries that use source paths, symbols, governance, coverage, source anchors,
or generated architecture remain source-requiring.

Keep the read-only LikeC4 custom text editor as the default editor for `.c4`
files. The normal text editor remains available through **Reopen Editor With**.
The preview renders only a named view owned by the open file. A file that owns
no view shows an explicit empty state. No renderer, protocol helper, or host
adapter selects a fallback view.

Keep one LikeC4 project under `docs/architecture/`. A domain file may contain
its model declarations and its primary named views. A separate view file is
for a cross-domain workflow or another view with no single model owner.
Renderer navigation follows a view's `sourcePath`, opens the owning file, and
keeps the host selector synchronized with the selected view.

The official LikeC4 editor extension remains optional. criv remains the
default preview, validation authority, State owner, and source-link provider.

## Consequences

The effective ADR set now has one query capability contract, one no-fallback
preview contract, and one architecture-file ownership contract.

This decision changes no command, State field, architecture artifact, or
editor behavior. It only makes the accepted governance graph describe the
behavior that later accepted decisions already selected and the code already
implements.

---
id: ADR-0096
kind: decision
title: Enforce Editor Preview Revision Lifecycle
status: accepted
date: 2026-08-11
governs:
  - extensions/vscode-criv/src/c4/preview.ts
  - extensions/vscode-criv/src/state/store.ts
  - .obsidian/plugins/criv/src/main.ts
policy:
  patterns:
    - id: vscode-custom-preview-needs-state-binding
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async resolveCustomTextEditor'
          - not:
              has:
                pattern: $STORE.onDidChangeStatus($$$_ARGS)
                stopBy: end
      message: Every VS Code custom C4 editor must subscribe to workspace State status changes.
    - id: vscode-command-preview-needs-state-binding
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async open'
          - has:
              pattern: vscode.window.createWebviewPanel($$$_CREATE_ARGS)
              stopBy: end
          - not:
              has:
                pattern: $STORE.onDidChangeStatus($$$_STATUS_ARGS)
                stopBy: end
      message: Every command-created VS Code C4 preview must subscribe to workspace State status changes.
    - id: vscode-preview-binding-needs-disposal
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - has:
              pattern: $STORE.onDidChangeStatus($$$_STATUS_ARGS)
              stopBy: end
          - not:
              has:
                pattern: $SUBSCRIPTION.dispose()
                stopBy: end
      message: A VS Code preview method that subscribes to State status must dispose its panel subscriptions.
    - id: obsidian-poll-needs-preview-publication
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async pollState'
          - not:
              has:
                pattern: this.refreshC4Views()
                stopBy: end
      message: Obsidian State polling must publish every changed State result to all open C4 previews.
    - id: obsidian-reload-needs-preview-publication
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async reloadState'
          - not:
              has:
                pattern: this.refreshC4Views()
                stopBy: end
      message: Obsidian manual State reload must publish the result to all open C4 previews.
    - id: obsidian-state-path-needs-lifecycle-reload
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async updateStatePath'
          - not:
              has:
                pattern: this.reloadState()
                stopBy: end
      message: An Obsidian State-path change must use the complete State and preview lifecycle.
---

# Enforce Editor Preview Revision Lifecycle

## Context

[[0083-own-one-loaded-state-revision-per-editor-workspace|ADR-0083]] requires
one loaded State revision for each editor workspace. The latest-started load
owns the result, a failed latest load clears the old revision, and shutdown
disposes active and late revisions.

[[0091-enforce-editor-adapter-boundaries|ADR-0091]] requires open C4 previews
to refresh after State changes. Commit `5503043` added refresh calls for
Obsidian polling and manual reload and State listeners for both VS Code preview
surfaces. GitHub issue #103 was created before that commit. Its original missing
bindings are therefore repaired, but structural checks alone do not define the
complete ready, invalid, recovery, late-render, and disposal contract.

The two editor hosts expose different preview surfaces. Obsidian can have many
open C4 leaves. VS Code has custom C4 editors and one command-created panel.
Every surface must consume the same workspace State lifecycle without moving
editor events or State file access into the shared LikeC4 renderer.

## Decision

Define one authoritative State-status stream for each editor workspace. The
workspace State host owns a monotonic status generation and publishes
`loading`, `ready`, `missing`, `invalid`, and `unavailable`. Every open preview
registers as a consumer when it opens and removes that registration when it
closes.

Keep the last valid diagram visible while a candidate State loads. Publish
`loading` only when no valid revision exists. A `ready` to `ready` transition
replaces every open preview with the new revision. A transition from `ready` to
`missing`, `invalid`, or `unavailable` disposes the renderer immediately and
replaces the diagram with the status message. Do not retain an old diagram
after the latest State load fails. Recovery to `ready` creates a renderer from
the new model.

Retain a selected view only when it exists in the new model. Otherwise select
the view owned by the open document. If the document owns no view, show the
explicit no-view state required by
[[0080-co-locate-primary-likec4-views-with-their-models|ADR-0080]].

Each preview owns its newest requested status generation. An asynchronous
render captures that generation and checks it again before it changes the
panel or leaf. A late render cannot replace a newer ready, invalid, missing, or
unavailable generation. Dispose a late candidate without publication. Prepare
a valid candidate before it replaces the visible renderer.

Preview close first invalidates pending renders, then removes the State
subscription, and then disposes the renderer exactly once. Workspace shutdown
stops new status publication before it disposes the active loaded State. Late
State and render candidates are also disposed exactly once.

Editor workspace hosts own State file observation, status generations,
refresh events, and subscription delivery. Preview adapters own the open
document, panel or leaf, status presentation, selected view, render
transaction, and cleanup. `@criv/editor-state` can own editor-neutral revision
and generation primitives, but it does not access editor files or surfaces.

The shared `@criv/likec4` package owns model rendering, view selection,
navigation callbacks, export, and renderer disposal. It receives an explicit
model and view. It does not load State or subscribe to editor events. Wasm owns
State validation and projections, not preview lifecycle. A later package
extraction decision remains with the package-boundary work; this decision does
not move host behavior into a shared package.

Add strict structural policies for each present editor preview entry point.
The VS Code custom editor and command-created panel must bind State status in
the method that establishes the preview lifecycle. A binding method must also
contain panel-subscription disposal. Obsidian polling, manual reload, and State
path changes must publish through the complete preview lifecycle.

The policies are deliberately local and exact. They prevent removal of a
required binding or cleanup path, but they cannot prove asynchronous ordering,
generation identity, complete status values, or exact disposal counts. Those
properties remain behavioral test requirements.

Run the same State-generation contract cases for both editor hosts. Tests
cover initial loading to ready; ready revision A to revision B; ready to
missing, invalid, and unavailable; recovery from each non-ready state; two
concurrent State loads; a late render after a newer ready or invalid result;
preview close during render; workspace shutdown during State load; and exact
single disposal of subscriptions, candidates, renderers, and loaded revisions.

Exercise many open Obsidian C4 leaves. Exercise the VS Code custom editor and
command-created panel separately. Verify view retention, document-owned view
selection, and the no-view state. Use distinct fake generations so a test
cannot pass by rendering the same model twice.

## Consequences

An open preview cannot continue to show a valid-looking old model after the
workspace State becomes invalid or disappears. Recovery updates every open
surface without requiring the user to reopen it.

Preview adapters have explicit generation and cleanup work. The shared
renderer stays independent from State files and editor events.

Structural enforcement makes missing host bindings fail `criv check` and
`criv enforce`. Lifecycle tests remain necessary because AST matching cannot
prove which asynchronous result wins or whether runtime resources are disposed
exactly once.

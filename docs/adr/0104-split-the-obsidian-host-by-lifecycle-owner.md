---
id: ADR-0104
kind: decision
title: Split The Obsidian Host By Lifecycle Owner
status: accepted
date: 2026-08-13
governs:
  - .obsidian/plugins/criv/src/**/*.ts
  - .obsidian/plugins/criv/test/**/*.mjs
policy:
  patterns:
    - id: no-obsidian-owner-imports-main
      language: typescript
      pattern: 'import $$$IMPORTS from "./main"'
      message: Obsidian lifecycle owners must use injected ports and must not import the composition module.
    - id: no-obsidian-owner-cross-import
      language: typescript
      rule: |
        all:
          - any:
              - pattern: import $$$IMPORTS from "./state-owner"
              - pattern: import $$$IMPORTS from "./c4-view"
              - pattern: import $$$IMPORTS from "./source-panel"
              - pattern: import $$$IMPORTS from "./source-references"
              - pattern: import $$$IMPORTS from "./settings"
          - not:
              inside:
                all:
                  - kind: program
                  - has:
                      pattern: class CrivPlugin extends Plugin { $$$BODY }
                      stopBy: end
                stopBy: end
      message: Only the Obsidian composition module can import a lifecycle owner; owners must use injected ports.
    - id: no-obsidian-main-runtime-ownership
      language: typescript
      rule: |
        all:
          - pattern: class CrivPlugin extends Plugin { $$$BODY }
          - has:
              any:
                - pattern: new LoadedRevisionOwner($$$ARGS)
                - pattern: new GenerationRevisionOwner($$$ARGS)
                - pattern: new CrivLikeC4Renderer($$$ARGS)
                - pattern: new RangeSetBuilder($$$ARGS)
                - pattern: ViewPlugin.fromClass($$$ARGS)
                - pattern: Decoration.mark($$$ARGS)
                - pattern: $APP.vault.adapter.read($PATH)
              stopBy: end
      message: Obsidian main.ts can compose owners but cannot own State revisions, C4 renderers, CodeMirror behavior, or State file reads.
---

# Split The Obsidian Host By Lifecycle Owner

## Context

The Obsidian adapter kept State polling, loaded revisions, commands, source
panels, C4 preview revisions, source hovers, suggestions, CodeMirror marks,
source preview rendering, and settings in one `main.ts` file. Changes to one
lifecycle could affect unrelated cleanup and revision state.

[[0096-enforce-editor-preview-revision-lifecycle|ADR-0096]] requires one State
status stream, generation-safe preview replacement, invalid-State clearing,
recovery, and exact disposal. The cross-host lifecycle behavior suite was made
green before this split. [[0099-enforce-shared-likec4-and-wasm-adapter-boundary|ADR-0099]]
keeps State semantics in Wasm and synchronous diagram rendering in the shared
LikeC4 renderer. Obsidian must still own its files, events, leaves, commands,
and cleanup.

## Decision

Split the Obsidian host by lifecycle owner:

- `main.ts` constructs owners, injects ports, registers Obsidian surfaces, and
  shuts owners down.
- `state-owner.ts` owns State file tokens, polling, one loaded Wasm revision,
  monotonic status generations, State commands, queries, and subscriptions.
- `c4-view.ts` owns the C4 `FileView`, source save actions, preview generations,
  view selection, navigation, export, and renderer disposal.
- `source-panel.ts` owns the singleton source-panel leaf and panel rendering.
- `source-references.ts` owns Markdown link decoration, hover lifecycle,
  selector suggestions, source navigation, and CodeMirror drift marks.
- `source-preview.ts` is a lifecycle-free utility for confined source reads,
  highlighting, and preview rendering.
- `settings.ts` owns settings data, persistence, and the settings tab.
- `ports.ts` owns the immutable State-status value and the narrow interfaces
  that the composition module injects.

The State owner publishes `generation`, `kind`, and a prepared projection or
error. A subscription returns a disposable handle. C4 views subscribe to that
stream. Source features use narrow State query methods. No feature owner gets
the complete `CrivPlugin` object.

The dependency direction is `main.ts` to lifecycle owners, then to
`core.ts`, `wasm.ts`, shared packages, Obsidian APIs, and lifecycle-free
utilities. A lifecycle owner must not import `main.ts` or another lifecycle
owner. The composition module connects owners through `ports.ts`.

Behavior tests use the public host seams. They prove State load ordering,
status publication, invalid-State clearing, recovery, many open C4 leaves,
view retention, missing-view behavior, late-render rejection, navigation, and
disposal. Tests must not only check that a file, import, or text value exists.
The inline ast-grep policies enforce only the structural ownership rules.

## Consequences

- `main.ts` contains composition and compatibility forwarding methods, not
  renderer, CodeMirror, or State file behavior.
- Each feature has one cleanup owner and one narrow dependency surface.
- The existing Obsidian behavior remains available while modules can change
  independently.
- Architecture views name the lifecycle owners and their runtime relations.

## Alternatives Considered

### Keep one plugin class

Rejected. It keeps unrelated revision and cleanup state in one owner.

### Pass the plugin object to every module

Rejected. It creates implicit owner-to-owner access and makes the split only a
file move.

### Put host lifecycle in shared packages

Rejected. Shared packages own editor-neutral State and renderer behavior.
Obsidian owns its events, files, leaves, commands, and settings.

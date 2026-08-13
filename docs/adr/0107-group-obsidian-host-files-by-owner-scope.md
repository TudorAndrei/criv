---
id: ADR-0107
kind: decision
title: Group Obsidian Host Files By Owner Scope
status: accepted
date: 2026-08-13
supersedes:
  - ADR-0104
governs:
  - .obsidian/plugins/criv/src/**/*.ts
  - .obsidian/plugins/criv/test/**/*.mjs
policy:
  patterns:
    - id: no-obsidian-owner-imports-main
      language: typescript
      rule: |
        any:
          - pattern: import $$$IMPORTS from "./main"
          - pattern: import $$$IMPORTS from "../main"
      message: Obsidian lifecycle owners must use injected ports and must not import the composition module.
    - id: no-obsidian-owner-cross-import
      language: typescript
      rule: |
        all:
          - any:
              - pattern: import $$$IMPORTS from "./state/owner"
              - pattern: import $$$IMPORTS from "../state/owner"
              - pattern: import $$$IMPORTS from "./owner"
              - pattern: import $$$IMPORTS from "./c4-view"
              - pattern: import $$$IMPORTS from "../c4-view"
              - pattern: import $$$IMPORTS from "./source/panel"
              - pattern: import $$$IMPORTS from "../source/panel"
              - pattern: import $$$IMPORTS from "./panel"
              - pattern: import $$$IMPORTS from "./source/references"
              - pattern: import $$$IMPORTS from "../source/references"
              - pattern: import $$$IMPORTS from "./references"
              - pattern: import $$$IMPORTS from "./settings"
              - pattern: import $$$IMPORTS from "../settings"
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

# Group Obsidian Host Files By Owner Scope

## Context

[[0104-split-the-obsidian-host-by-lifecycle-owner|ADR-0104]] defines the
Obsidian lifecycle owners and their dependency direction. Those owners used
flat files at the plugin source root. Source work and State work each contain
several files, so their flat names hide their owner scope.

The LikeC4 view and settings owners each have one file. A directory for each
single file would add no useful scope.

## Decision

Keep the Obsidian package root at `.obsidian/plugins/criv`. Group only real
multi-file owner scopes.

The `state/` directory owns the State lifecycle and Wasm adapter. The `source/`
directory owns source models, panels, previews, and references. `main.ts`
remains the composition module. `ports.ts`, `settings.ts`, and `c4-view.ts`
remain root leaf modules.

Use short filenames inside a scope. Do not add `index.ts` barrels. Keep the
dependency direction from ADR-0104: `main.ts` imports lifecycle owners, owners
use injected ports, and one lifecycle owner does not import another owner.

Behavior tests use the owner interfaces. Structural policy scans all nested
TypeScript files.

## Consequences

The file tree shows the State and Source owners without changing Obsidian
commands, settings, view identifiers, State behavior, or cleanup behavior.
Accepted policy patterns name the current import paths.

## Alternatives Considered

### Keep all host files flat

Rejected. The State and Source owners contain several related files and need a
visible scope.

### Add one directory for every owner

Rejected. One-file directories add depth to the tree without adding a module
scope.

### Move shared packages under the plugin

Rejected. `@criv/editor-state` and `@criv/likec4` are editor-neutral packages,
not Obsidian lifecycle owners.

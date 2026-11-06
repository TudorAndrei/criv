---
id: ADR-0172
kind: decision
title: Retain the Obsidian owner layout with TypeScript tests
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0107
governs:
  - .obsidian/plugins/criv/src/**/*.ts
  - .obsidian/plugins/criv/test/**/*.ts
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
                - pattern: new CrivStateStream($$$ARGS)
                - pattern: new RangeSetBuilder($$$ARGS)
                - pattern: ViewPlugin.fromClass($$$ARGS)
                - pattern: Decoration.mark($$$ARGS)
                - pattern: $APP.vault.adapter.read($PATH)
              stopBy: end
      message: Obsidian main.ts can compose owners but cannot own the State stream, CodeMirror behavior, or State file reads.
---

# Retain the Obsidian Owner Layout with TypeScript Tests

## Context

[[0107-group-obsidian-host-files-by-owner-scope|ADR-0107]] groups the
Obsidian host by owner scope and enforces the import direction with inline
policies. It governs `.mjs` test scripts, which are now TypeScript tests, and
its policies name the retired `c4-view` owner and revision owners.
[[0170-share-editor-state-behaviour-and-embed-the-wasm-runtime|ADR-0170]]
moved the State status stream into `@criv/editor-state`.

## Decision

Retain the complete owner layout and dependency direction of ADR-0107. `main.ts`
composes owners. The `state/` scope owns the State file, polling, and the State
stream. The `source/` scope owns source models, panels, previews, and
references. `ports.ts` owns the narrow `StatePort` and `SourceResolver`
interfaces; the State status value is the shared package type.

Govern the TypeScript tests. Drop the retired `c4-view` import paths from the
policies. The composition module cannot construct the State stream.

## Consequences

The owner layout and its structural checks continue unchanged for the current
files.

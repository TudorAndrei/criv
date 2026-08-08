---
id: ADR-0091
kind: decision
title: Enforce Editor Adapter Boundaries
status: accepted
date: 2026-08-08
governs:
  - crates/criv-wasm/src/lib.rs
  - extensions/vscode-criv/src/**/*.ts
  - .obsidian/plugins/criv/src/**/*.ts
policy:
  patterns:
    - id: no-state-pattern-fallback
      language: typescript
      rule: |
        any:
          - pattern: Object.keys($OBJ.patterns ?? {})
          - pattern: Object.keys($OBJ?.patterns ?? {})
      message: ADR-0088 requires registered-patterns from the validated State contract; do not derive it from match keys.
    - id: no-editor-c4-source-parser
      language: typescript
      rule: |
        pattern: |
          export function parseC4Artifact($$$ARGS): $RET { $$$BODY }
      message: ADR-0074 keeps C4 parsing and validation in the shared LikeC4 package, not in editor adapters.
    - id: no-editor-state-schema-literal
      language: typescript
      rule: |
        pattern: '"criv.state.v1"'
      message: ADR-0088 keeps State schema validation in the Rust-Wasm boundary.
    - id: no-direct-likec4-host-import
      language: typescript
      rule: |
        pattern: import $$$IMPORTS from "likec4"
      message: ADR-0074 requires editor hosts to use the shared LikeC4 package.
    - id: no-likec4-fallback-identifier
      language: typescript
      rule: |
        kind: identifier
        regex: '^(defaultLikeC4ViewId|fallbackLikeC4ViewId)$'
      message: ADR-0080 forbids fallback LikeC4 views when the declared view is absent.
    - id: no-editor-source-lookup-index
      language: typescript
      rule: |
        kind: identifier
        regex: '^(buildSourceTargetIndex|resolveSourceEntry|canonicalByLegacy)$'
      message: ADR-0071 keeps source-target lookup and legacy alias resolution in the canonical Wasm revision.
    - id: preview-needs-state-binding
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - has:
              pattern: vscode.window.createWebviewPanel($$$_CREATE_ARGS)
              stopBy: end
          - not:
              has:
                pattern: $STORE.onDidChangeStatus($$$_STATUS_ARGS)
                stopBy: end
      message: ADR-0083 requires an open VS Code preview to refresh when State changes.
    - id: poll-needs-preview-refresh
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async pollState'
          - not:
              has:
                pattern: this.refreshC4Views()
                stopBy: end
      message: ADR-0083 requires open Obsidian previews to refresh when State changes.
---

# Enforce Editor Adapter Boundaries

## Context

The accepted-ADR compliance audit found duplicate C4 parsing, State contract
fallbacks, host-side source lookup, and previews that did not refresh after a State change. These paths
conflicted with [[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]],
[[0080-co-locate-primary-likec4-views-with-their-models|ADR-0080]],
[[0083-own-one-loaded-state-revision-per-editor-workspace|ADR-0083]], and
[[0088-share-the-state-wire-document|ADR-0088]].

The fixes remove the duplicate parser and fallback behavior. Both editor
adapters now refresh open previews from State lifecycle events. Structural
policies can prevent the same syntax from returning.

Ast-grep cannot prove that every State value is correct or that every preview
renders the correct result. Tests remain responsible for those behaviors.

## Decision

Add accepted inline policies for the TypeScript editor adapters.

Editor adapters must not derive registered patterns from State match keys.
They must not validate the raw State schema string. State validation stays in
the Rust-Wasm contract.

Editor adapters must not own an exported C4 artifact parser or import the
LikeC4 dependency directly. They use the shared LikeC4 package. They must not
introduce a default or fallback LikeC4 view identifier.

The active Wasm revision owns source-target lookup. It resolves exact targets
and unique legacy symbol or basename aliases. It rejects an ambiguous legacy
alias. Editor adapters may associate the returned canonical node with a
prepared source entry, but they must not build another lookup or alias index.

A VS Code method that creates the preview panel must bind a State status
listener in that method. The Obsidian State polling method must refresh open C4
views after a change.

Each policy is a prohibited syntax pattern or a required local binding. An
absent match is not proof of complete behavioral compliance.

## Consequences

`criv check` and `criv enforce` reject common editor-side bypasses of the State
and LikeC4 boundaries.

The shared TypeScript packages are outside this policy scope. They can import
LikeC4 and can own protocol parsing because the accepted ADRs assign those
responsibilities to them.

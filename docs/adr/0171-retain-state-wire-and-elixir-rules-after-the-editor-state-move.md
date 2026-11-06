---
id: ADR-0171
kind: decision
title: Retain State wire and Elixir rules after the editor-state move
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0088
  - ADR-0119
governs:
  - Cargo.toml
  - Cargo.lock
  - criv.toml
  - crates/criv-state-wire/Cargo.toml
  - crates/criv-state-wire/src/lib.rs
  - crates/criv-wasm/Cargo.toml
  - crates/criv-wasm/src/lib.rs
  - crates/criv-wasm/src/source.rs
  - src/state.rs
  - src/state/snapshots.rs
  - src/init/templates.rs
  - src/source/graph.rs
  - src/structural.rs
  - src/query.rs
  - scripts/performance/Cargo.toml
  - scripts/performance/src/bin/criv-state-storage-baseline.rs
  - scripts/performance/state-store-prototype/Cargo.toml
  - scripts/performance/state-store-prototype/src/lib.rs
  - fixtures/editor/source-target-lookup.v1.json
  - packages/criv-editor-state/src/*.ts
  - extensions/vscode-criv/src/state/store.ts
  - extensions/vscode-criv/src/navigation/languageFeatures.ts
  - extensions/vscode-criv/src/navigation/references.ts
  - .obsidian/plugins/criv/src/state/owner.ts
  - .obsidian/plugins/criv/src/source/model.ts
  - .obsidian/plugins/criv/src/source/preview.ts
  - .obsidian/plugins/criv/src/main.ts
---

# Retain State Wire and Elixir Rules After the Editor-State Move

## Context

[[0170-share-editor-state-behaviour-and-embed-the-wasm-runtime|ADR-0170]]
moved the editor Wasm adapters and the source-target rules into
`@criv/editor-state`. The files `extensions/vscode-criv/src/state/wasm.ts`,
`.obsidian/plugins/criv/src/state/wasm.ts`, and
`extensions/vscode-criv/src/navigation/target.ts` no longer exist.
[[0088-share-the-state-wire-document|ADR-0088]] and
[[0119-first-class-elixir-language-support|ADR-0119]] govern those deleted
paths. Their accepted bytes are immutable.

## Decision

Retain the complete decisions of ADR-0088 and ADR-0119. Their rules remain
normative by reference. This successor changes only the governed paths.

The State wire rules of ADR-0088 apply to the shared editor package in place of
the two deleted editor `wasm.ts` adapters.

The Elixir rules of ADR-0119 apply to the shared target rules in
`@criv/editor-state` in place of the deleted VS Code `target.ts`. Elixir
selectors resolve through the same Wasm lookup as before.

## Consequences

The governed paths name files that exist. The State wire and Elixir behaviour
does not change.

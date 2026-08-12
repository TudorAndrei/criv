---
id: ADR-0099
kind: decision
title: Enforce The Shared LikeC4 And Wasm Adapter Boundary
status: accepted
date: 2026-08-12
supersedes:
  - ADR-0074
governs:
  - src/c4.rs
  - src/c4_code.rs
  - src/likec4.rs
  - src/architecture.rs
  - src/check.rs
  - src/source_graph.rs
  - src/state.rs
  - src/vault.rs
  - crates/criv-wasm/src/lib.rs
  - packages/criv-likec4/**
  - packages/criv-editor-state/**
  - .obsidian/plugins/criv/**
  - extensions/vscode-criv/**
  - assets/likec4-bridge.mjs
  - assets/likec4-contract.json
  - package.json
  - package-lock.json
  - mise.toml
policy:
  patterns:
    - id: no-likec4-root-import
      language: typescript
      pattern: 'import $$$IMPORTS from "@criv/likec4"'
      message: Import an explicit @criv/likec4 protocol or renderer entry.
    - id: no-likec4-node-import
      language: typescript
      pattern: 'import $$$IMPORTS from "@criv/likec4/node"'
      message: The embedded criv bridge is the only Node.js LikeC4 compiler.
    - id: no-host-architecture-model-builder
      language: typescript
      rule: |
        any:
          - pattern: function architectureModel($$$ARGS) { $$$BODY }
          - pattern: function c4Artifacts($$$ARGS) { $$$BODY }
          - pattern: function registeredPatterns($$$ARGS) { $$$BODY }
      message: Wasm returns architecture, C4 artifacts, and registered patterns as ready editor projections.
    - id: no-host-raw-likec4-read
      language: typescript
      pattern: $ARCHITECTURE.model.raw
      message: Editor hosts must pass the Wasm LikeC4 projection without reading the raw State model.
    - id: no-likec4-version-cast
      language: typescript
      pattern: '$VALUE as "1.59.2"'
      message: Wasm validates the LikeC4 version; a TypeScript cast cannot establish the contract.
    - id: obsidian-c4-close-needs-renderer-disposal
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async onClose'
          - not:
              has:
                pattern: this.previewRevisions.dispose()
                stopBy: end
      message: Closing an Obsidian C4 leaf must dispose its complete preview revision lifecycle.
---

# Enforce The Shared LikeC4 And Wasm Adapter Boundary

## Context

[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] selected one
LikeC4 workspace, a criv-owned Node.js bridge, a shared browser renderer, and
thin editor adapters. The package boundary did not make that ownership strict.
`@criv/likec4` exported an unused Node.js compiler with behavior that differed
from the embedded bridge. It also had no direct test or type gate.

[[0088-share-the-state-wire-document|ADR-0088]] makes Rust-Wasm the canonical
editor projection owner. Both hosts still read State fields and built parts of
the architecture projection. The same State could therefore produce different
editor results.

## Decision

Retain all architecture-source, dependency, offline, bridge, State,
renderer, navigation, and host-adapter decisions from ADR-0074, except for the
Node.js compiler assignment described below. This ADR is its complete
successor.

Keep `assets/likec4-bridge.mjs` as the only Node.js LikeC4 compiler. Rust starts
the bridge in the repository, checks process limits, and validates its output.
Remove the unused `@criv/likec4/node` entry and the package root entry. The
shared package exports only `@criv/likec4/protocol` and
`@criv/likec4/renderer`.

Use `assets/likec4-contract.json` as the only checked-in owner for the criv
LikeC4 protocol version, required Node.js version, and required LikeC4 version.
Rust reads it directly. TypeScript imports it. Rust creates the embedded bridge
source from it. Direct tests reject a version copy that does not agree with the
contract.

Keep the serialized State document in `criv-state-wire`. Keep all State
validation and editor projection meaning in `criv-wasm`. A loaded revision
returns summary, safe sources, graph nodes, registered patterns, pattern
matches, optional architecture, and C4 artifacts. It does not return the raw
State document.

An absent architecture value is valid. A present architecture value must have
the supported protocol and LikeC4 versions, a safe workspace, a valid raw
LikeC4 model, named views, and validated source targets. Any invalid present
value rejects the complete State revision. Wasm returns a complete
`CrivLikeC4Model`. A host passes that value to the renderer without conversion,
filtering, fallback values, or version casts.

Use stable codes for invalid JSON, unsupported State schema, invalid
architecture wrapper, unsupported criv LikeC4 protocol, unsupported LikeC4
version, invalid raw LikeC4 model, unavailable Wasm, and use after disposal.
The shared adapter owns each code and base message. A host can add context and
a recovery action, but it must not report invalid State as absent architecture.

Keep `@criv/editor-state` editor-neutral. It owns the injected Wasm loader,
stable errors, Wasm-call adapter, generation ordering, replacement, and exact
disposal. It does not own State row types, State projection, LikeC4 behavior,
files, editor APIs, status text, or a generated Wasm import.

Keep renderer work in `@criv/likec4/renderer`. It receives an explicit prepared
model and an existing view. It owns synchronous replacement, view selection,
navigation callbacks, validated source callbacks, SVG export, React unmount,
and idempotent disposal. It does not own asynchronous generation order. An
unknown view and every operation after disposal fail with stable errors.

Keep State file access, generated Wasm imports, editor events, status streams,
panels, leaves, commands, and host cleanup in each editor package. A host can
adapt prepared rows to controls, trees, messages, and file targets. It cannot
inspect raw State or rebuild semantic projections.

Add `npm run check:editor-contracts` at the repository root. It runs strict
type checks and direct tests for both shared packages and the Rust-Wasm
contract. Both host builds and tests build their own Wasm target first, run the
shared contract command, then build and test host behavior. The VS Code
prepublish and release package paths run the same gate from a clean checkout.

Direct tests cover package exports, version identity, a shared architecture
fixture, valid and invalid State, view ownership, renderer replacement,
navigation, SVG export, disposal, and both generated Wasm loaders. Host tests
cover State status changes, newest-generation publication, recovery, view
selection, navigation, close, and host-specific cleanup. Do not add a fallback
or compatibility layer.

Enforce the common bypass forms as inline policies. Direct package tests also
reject forbidden dependencies that structural patterns cannot safely scope to
one package.

The implementation is owned by `src/likec4.rs`,
`crates/criv-wasm/src/lib.rs#fn:prepare_architecture`,
`packages/criv-likec4/src/protocol.ts`,
`packages/criv-likec4/src/renderer.ts`, and
`packages/criv-editor-state/src/wasmHost.ts`.

## Consequences

Both editors receive the same validated architecture and State-derived lists.
The public package paths and direct gates now fail before a host bundle can hide
shared contract drift.

The Wasm projection is larger, and both hosts must build their target-specific
Wasm loaders before their contract and host checks. The embedded bridge stays a
self-contained deployment asset because it runs inside user repositories.

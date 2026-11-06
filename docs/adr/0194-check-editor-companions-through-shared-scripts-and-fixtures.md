---
id: ADR-0194
kind: decision
title: Check editor companions through shared scripts and fixtures
status: accepted
date: 2026-11-06
governs:
  - package.json
  - .github/workflows/ci.yml
  - .github/workflows/release.yml
  - extensions/vscode-criv/.vscodeignore
  - extensions/vscode-criv/scripts/build-tests.mjs
  - fixtures/editor/check-diagnostics.v1.json
  - fixtures/editor/node-kinds.v1.json
  - src/check.rs
  - crates/criv-state-wire/src/lib.rs
  - packages/criv-editor-state/src/contract.ts
  - extensions/vscode-criv/src/diagnostics/model.ts
---

# Check Editor Companions Through Shared Scripts and Fixtures

## Context

[[0103-required-repository-self-governance|ADR-0103]] gives the shared editor
contract lane the shared TypeScript packages and the `criv-wasm` tests. The
`criv-wasm` tests also ran in the workspace `cargo nextest` run of the core
lane and the Windows lane, so `check:editor-contracts` ran them a third time.
A separate `Wasm build` lane built the Wasm package that the shared package
test already builds. The release quality job repeated each companion CI step
by hand, so a step added to one workflow was not in the other.

Two editor types copied a Rust shape with no test between them. The VS Code
reader of `criv check --format json` copied the `JsonDiagnostic` fields in
`src/check.rs`. `CrivNodeKind` in `@criv/editor-state` copied the wire
`NodeKind` names. The VS Code unit tests resolved source targets through a
hand-written lookup instead of the Wasm runtime that the extension ships.

The VSIX file list was a denylist. A stale build directory in the extension
folder, such as an old `media/`, went into a local package.

## Decision

The root `package.json` owns one script for each editor lane:

- `check:editor-contracts` checks and tests `@criv/editor-state`, which builds
  the Wasm package. It does not run Rust tests.
- `check:obsidian` audits, format-checks, lints, tests, and builds the
  Obsidian companion.
- `check:vscode` audits, format-checks, lints, and unit-tests the VS Code
  companion, then runs its extension-host integration tests.

The CI lanes and the release quality job call these scripts. CI and release
run `check:vscode` under Xvfb. There is no separate Wasm build lane. The
`criv-wasm` Rust tests run in the workspace `cargo nextest` run. This changes
only the ADR-0103 lane split; the rest of ADR-0103 is unchanged.

A shape that Rust writes and an editor reads has one fixture under
`fixtures/editor/`. A Rust test in the module that owns the shape compares
the shape with the fixture, and an editor test reads the same fixture:

- `check-diagnostics.v1.json` is the output of `JsonDiagnostic` in
  `src/check.rs`. The VS Code diagnostics test parses it.
- `node-kinds.v1.json` lists the `NodeKind` wire names of `criv-state-wire`.
  A `@criv/editor-state` test requires `CrivNodeKind` to name the same list.

The VS Code unit tests bundle the embedded Wasm runtime and resolve targets in
`fixtures/link-resolution.json` and `fixtures/editor/source-target-lookup.v1.json`
through it, as the Obsidian tests do under
[[0170-share-editor-state-behaviour-and-embed-the-wasm-runtime|ADR-0170]].

`.vscodeignore` is an allowlist: the manifest, `README.md`, `LICENSE`, the
icon, and `dist/extension.js`.

## Consequences

A companion check changes in one place for CI and release. CI runs fewer
lanes, and each Rust test runs once in each workspace run.

A change to the check JSON or to the node kinds fails a Rust test until the
fixture changes, and then fails an editor test until the editor type changes.

A new file for the VSIX must be added to the allowlist.

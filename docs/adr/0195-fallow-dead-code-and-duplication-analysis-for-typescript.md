---
id: ADR-0195
kind: decision
title: Fallow dead-code and duplication analysis for TypeScript
status: accepted
date: 2026-11-06
governs:
  - .fallowrc.json
  - package.json
  - hk.pkl
  - packages/criv-editor-state/esbuild.mjs
  - extensions/vscode-criv/scripts/build-tests.mjs
  - .obsidian/plugins/criv/scripts/build-tests.mjs
  - .obsidian/plugins/criv/package.json
---

# Fallow Dead-Code and Duplication Analysis for TypeScript

## Context

[[0043-hawk-visibility-analysis|ADR-0043]] makes Hawk reject unused Rust
visibility. The TypeScript in `@criv/editor-state`, the VS Code extension, and
the Obsidian plugin had no equivalent. oxlint checks one file at a time, so it
does not find an unused file, an unused export, an unused class member, an
unlisted dependency, or code copied between packages.

A first fallow run found a real hidden dependency. The Obsidian plugin imports
`@codemirror/state` and `@codemirror/view`, which `obsidian` declares only as
peer dependencies. It also found that the two host `build-tests.mjs` scripts
copied the same test-entry walk and esbuild call.

## Decision

Pin `fallow` as an exact root devDependency. The root `check:fallow` script
runs `fallow dead-code` and `fallow dupes`. Both exit non-zero on a finding.
`check:editor-contracts` runs `check:fallow` first, so the CI and release
editor-contract lanes of
[[0194-check-editor-companions-through-shared-scripts-and-fixtures|ADR-0194]]
run it.

The pre-push hook runs `check:fallow` when a companion package, the root npm
manifests, or `.fallowrc.json` change. The analysis takes less than a second
and needs no build. It needs the root `npm ci`. It is not in the core `check`
hook, because the core lane does not install npm packages. This adds one step
to the pre-push set of
[[0061-hook-owned-local-validation-and-direct-ci-profile|ADR-0061]], for the
reason that ADR-0060 added Hawk: a deterministic CI failure must fail before
the push.

`.fallowrc.json` analyses only the three companion packages:

- fallow skips hidden directories, so the config lifts the Obsidian plugin
  sources, tests, scripts, and manifests. The plugin bundle `main.js` stays
  out.
- The host test files are entry points, because the host `build-tests.mjs`
  scripts find them at run time.
- The Obsidian test stubs stay out. esbuild loads them through an alias that
  replaces `obsidian` and CodeMirror only in the test bundle.
- `usedClassMembers` names the members that a runtime calls through a
  contract: `dispose` on a `vscode.Disposable`, `onDidChangeTreeData` on a
  `vscode.TreeDataProvider`, and the `code` of an `Error` subclass, which
  callers read as part of the `@criv/editor-state` error contract.

The Obsidian plugin lists `@codemirror/state` and `@codemirror/view` as
`devDependencies` at the versions that `obsidian` requires. `bundleTests` in
`@criv/editor-state/esbuild` owns the host test bundle: the entry walk, the
output reset, and the Wasm plugin. Each host script gives only its suite,
target, externals, and aliases.

`fallow health` reports complexity. It is advisory and is not a gate.

## Consequences

An unused file, export, class member, or dependency in a companion package,
or a copied block across them, fails pre-push and the editor-contract lanes.

A new runtime-called member that fallow cannot trace needs a
`usedClassMembers` entry. A new test entry that a script finds at run time
needs an `entry` pattern.

Updating fallow is a root `package.json` change.

---
id: ADR-0035
kind: decision
title: VS Code-compatible companion extension
status: accepted
date: 2026-06-24
governs:
  - extensions/vscode-criv/package.json
  - extensions/vscode-criv/src/extension.ts
  - crates/criv-wasm/src/lib.rs
  - mise.toml
---

# VS Code-compatible Companion Extension

## Context

criv already has an Obsidian companion plugin that reads generated criv state
and improves local authoring ergonomics without becoming a second
implementation of graph generation, source validation, or policy enforcement.
The same boundary is needed for VS Code and VS Code-derived editors such as
Cursor.

The extension should support common VS Code-compatible editors, not only
Microsoft VS Code. Marketplace availability varies by editor, and non-Microsoft
editors may prefer VSIX or Open VSX distribution. The implementation therefore
needs to stay on stable VS Code APIs and avoid proposed APIs or
Microsoft-host-specific behavior.

## Decision

Create a VS Code-compatible extension package under
`extensions/vscode-criv/package.json`. The extension is a workspace extension
that consumes `criv.toml`, `.criv/state.json`, Markdown files, and standalone
`.c4` artifacts from the current repository. It may run local criv commands
only through explicit user actions and workspace-trust-aware command paths.

The extension should use the stable VS Code Extension API, TypeScript, esbuild,
`tsc --noEmit`, `@vscode/test-cli`, and `@vscode/test-electron`. The initial
compatibility floor is VS Code `^1.85.0`: old enough for broad VS Code-like
editor support, but new enough to avoid relying on legacy activation behavior.
The extension must avoid proposed API declarations.

The extension package should be distributable as a local `.vsix` before any
marketplace release. Metadata should remain suitable for later publication to
both the VS Code Marketplace and Open VSX. `criv init` may recommend the
extension through `.vscode/extensions.json` once the package ID is stable, but
default initialization must not install an extension into a user's editor
environment.

The Rust CLI remains authoritative for generated state, validation, source
selectors, C4 artifact checks, and enforcement. The VS Code-compatible extension
is a state consumer and renderer. Rust/WASM helpers in
`crates/criv-wasm/src/lib.rs` may provide editor-independent parsing and
summary functions, but they must not depend on VS Code APIs or perform
filesystem or process operations.

`.c4` rendering in the extension is a projection over text-first architecture
artifacts. It may use webviews, Mermaid, Viz.js, and shared parser/sanitizer
helpers, but rendered diagrams are never authoritative source. `criv check`
remains the validator.

## Consequences

The extension can be installed in VS Code-compatible editors through VSIX before
marketplace publication. Cursor compatibility must be verified with packaged
VSIX smoke tests rather than inferred only from VS Code tests.

Keeping `criv init` to recommendations by default preserves repository-local
idempotence and avoids surprising user-level editor mutations. An explicit
future install flag can target `code`, `cursor`, or a configured editor CLI if
that workflow becomes important.

The extension will duplicate some editor UI behavior from the Obsidian plugin,
but the authoritative graph and validation boundary stays in the CLI and shared
WASM helpers.

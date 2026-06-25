---
id: ADR-0037
kind: decision
title: VS Code JSON diagnostics in hooks
status: accepted
date: 2026-06-25
governs:
  - hk.pkl
  - mise.toml
  - extensions/vscode-criv/package.json
  - extensions/vscode-criv/language-configuration.json
  - extensions/vscode-criv/test/integration/runner.ts
---

# VS Code JSON Diagnostics in Hooks

## Context

[[0035-vscode-compatible-companion-extension|ADR-0035]] created the VS
Code-compatible companion extension, and
[[0036-vscode-extension-test-stack|ADR-0036]] selected Node unit tests plus
`@vscode/test-electron` for extension-host coverage.

VS Code reports extension manifest and language configuration problems through
its built-in JSON language features. These diagnostics include extension-aware
warnings such as generated activation events, missing view icons, and schema
shape errors in `language-configuration.json`.

Generic TypeScript linting, `oxlint`, and `vsce package` do not catch the same
editor diagnostics. A project-specific custom linter would duplicate VS Code
rules and drift from editor behavior.

The JavaScript toolchain should also use a production LTS runtime. As of
2026-06-25, Node 24 is the active LTS line, while Node 26 is the current
non-LTS line.

## Decision

Run VS Code's own JSON diagnostics for extension manifest and language
configuration files through the existing `@vscode/test-electron` integration
harness.

Expose that check as `mise run vscode-json-diagnostics`, and run it from
`hk.pkl` in `pre-commit` and `check` when VS Code extension manifest,
language-configuration, or integration test files change.

Keep `oxlint` as the source linter for TypeScript and JavaScript files. Do not
add a criv-specific custom manifest linter for VS Code package metadata.

Pin the repository Node tool in `mise.toml` to the active LTS major line
(`node = "24"`) for the JavaScript toolchain.

## Consequences

The hook path catches the same VS Code JSON warnings developers see in the
editor before committing extension manifest or language configuration changes.

The diagnostics check is slower than a pure JSON Schema CLI because it launches
a VS Code test host, so it is scoped to the files that can produce these editor
diagnostics.

Using the VS Code integration harness keeps the validation aligned with the
installed VS Code behavior and avoids maintaining local copies of extension
manifest rules.

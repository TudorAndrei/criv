---
id: ADR-0036
kind: decision
title: VS Code extension test stack
status: accepted
date: 2026-06-25
supersedes:
  - ADR-0035
governs:
  - extensions/vscode-criv/package.json
  - extensions/vscode-criv/test/unit/*.test.ts
  - extensions/vscode-criv/test/integration/runner.ts
---

# VS Code Extension Test Stack

## Context

[[0035-vscode-compatible-companion-extension|ADR-0035]] selected the stable VS
Code Extension API and named both `@vscode/test-cli` and
`@vscode/test-electron` for extension tests.

During implementation, `@vscode/test-cli` introduced an avoidable high-severity
npm audit finding through its dependency tree. The extension also needs fast
unit coverage for pure parsing, source-selector, command-runner, and webview
HTML helpers that do not need a VS Code extension host.

## Decision

Use Node's built-in test runner for unit tests over pure TypeScript helper
modules. Bundle those tests with esbuild before running them under `node --test`.

Use `@vscode/test-electron` only for extension-host integration tests that need
VS Code APIs, such as command registration and activation smoke coverage.

Do not depend on `@vscode/test-cli` unless a later audit-clean version provides
coverage that cannot be matched by the lighter Node plus `@vscode/test-electron`
workflow.

## Consequences

The extension keeps broad VS Code-compatible editor support from ADR-0035 while
avoiding an unnecessary vulnerable test dependency.

Unit tests stay fast and do not require downloading or launching a VS Code test
host. Integration tests still cover extension activation in a real VS Code host
when explicitly run.

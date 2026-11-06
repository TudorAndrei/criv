---
id: ADR-0185
kind: decision
title: VS Code unit tests by feature directory
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0036
governs:
  - extensions/vscode-criv/package.json
  - extensions/vscode-criv/test/unit/**/*.test.ts
  - extensions/vscode-criv/test/integration/runner.ts
---

# VS Code Unit Tests by Feature Directory

## Context

[[0036-vscode-extension-test-stack|ADR-0036]] chose Node's built-in test runner
for pure helper modules and `@vscode/test-electron` for extension-host tests.
Its scope named only unit tests directly under `test/unit/`.

Asset lookup moved into `@criv/editor-state`, so the last unit test at that
level moved with it. The remaining unit tests sit in feature directories such as
`test/unit/navigation/` and `test/unit/diagnostics/`.

## Decision

Keep the ADR-0036 test stack unchanged: bundle unit tests with esbuild and run
them under `node --test`, and use `@vscode/test-electron` only for tests that
need the VS Code extension host. Do not depend on `@vscode/test-cli`.

Place each unit test in the directory of the feature it covers, under
`extensions/vscode-criv/test/unit/`.

## Consequences

The decision covers unit tests at any depth below `test/unit/`, so moving a test
between feature directories does not leave the decision without a scope.

The test commands and dependencies stay as ADR-0036 set them.

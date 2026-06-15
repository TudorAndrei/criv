---
id: ADR-0024
kind: decision
title: Oxlint Only JavaScript TypeScript Enforcement
status: accepted
date: 2026-06-16
governs:
  - src/enforce.rs
  - .obsidian/plugins/criv/package.json
  - .obsidian/plugins/criv/package-lock.json
---

# Oxlint Only JavaScript TypeScript Enforcement

## Context

The repository contains authored JavaScript and TypeScript primarily for the
Obsidian plugin governed by
[[0009-obsidian-plugin-as-state-consumer|ADR-0009]] and
[[0023-do-not-track-generated-plugin-bundles|ADR-0023]].

The plugin package already pins `oxlint` and exposes `npm run lint` through
`.obsidian/plugins/criv/package.json`. The repository does not pin ESLint, does
not carry an ESLint configuration, and does not use ESLint as part of the plugin
development workflow.

Keeping an ESLint fallback in criv enforcement makes the repository policy look
ambiguous and produces misleading skipped-tool output when the correct local
tool is installed under the plugin package.

## Decision

Use oxlint as the only JavaScript and TypeScript lint tool for this repository.

`criv enforce` should run oxlint for JavaScript and TypeScript files when
oxlint is available from the repository root, the Obsidian plugin package, or
`PATH`. It should not attempt to run ESLint.

When oxlint is unavailable, `criv enforce` may preserve the existing optional
native-tool behavior and report that oxlint was skipped. That skip means the
configured oxlint executable is missing; it must not fall through to ESLint.

## Consequences

The native enforcement output matches the repository's actual toolchain.

Plugin linting remains aligned with package scripts and pinned dependencies.
Repository-level CI should install plugin dependencies before enforcement when
JavaScript or TypeScript linting is expected.

Any future move away from oxlint requires a new ADR because accepted ADRs are
immutable and this decision intentionally removes linter ambiguity.

---
id: ADR-0084
kind: decision
title: Require Windows Hosted Validation
status: accepted
date: 2026-08-07
supersedes:
  - ADR-0061
governs:
  - .github/workflows/ci.yml
---

# Require Windows Hosted Validation

## Context

[[0061-hook-owned-local-validation-and-direct-ci-profile|ADR-0061]] keeps the
Windows build-and-test job visible but non-gating. That temporary exception
let the project add Windows coverage before it fixed the failures recorded in
issues #27 and #28. Both issues are now closed. A later performance prototype
also lacked its required Windows peak-memory implementation; the gate change
includes that direct portability fix.

The stable `Repository checks` aggregate is the branch-protection surface. A
required Windows job outside that aggregate could fail while the protected
status still succeeds.

## Decision

Make the Windows build-and-test job a required hosted validation lane. Remove
its job-level `continue-on-error` setting, make `Repository checks` depend on
the Windows job, and require the Windows result to be `success`.

This decision supersedes ADR-0061 only for the Windows job status. All other
hosted and local validation decisions in ADR-0061 remain in force. Do not add
more Windows targets or change the priority of Windows-specific defects as
part of this decision.

## Consequences

A failed or cancelled Windows build or test now fails the stable aggregate and
blocks a protected merge. The aggregate waits for five lanes instead of four,
so Windows runner availability and duration are now part of the merge gate.

Linux, Wasm, Obsidian, VS Code, and local hook behavior do not change.

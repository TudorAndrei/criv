---
id: ADR-0055
kind: decision
title: Dependency Auditing In hk Checks
status: accepted
date: 2026-08-01
governs:
  - hk.pkl
  - .obsidian/plugins/criv/package-lock.json
  - extensions/vscode-criv/package-lock.json
---

# Dependency Auditing In hk Checks

## Context

[[0049-checks-defined-in-hk-not-mise|ADR-0049]] assigns repository check
commands to `hk.pkl`. The former check suite installed `cargo-audit` through
`mise.toml` but did not invoke it, and neither JavaScript lockfile was audited.

The Rust posture in [[dependency-evaluations]] is intentionally not a failing
gate: `cargo audit --no-fetch` reads a dated local advisory database and cannot
yet provide a reproducible CI advisory feed. In contrast, npm has a hosted
advisory service and both companion packages are installed reproducibly with
`npm ci` in `.github/workflows/ci.yml`.

## Decision

The full hk check always runs the three steps in `hk.pkl#L127-L137` and
`hk.pkl#L187-L209`:

- `cargo-audit` runs `cargo audit --no-fetch`. Its output is retained, but every
  non-zero exit code is reported and allowed; it remains monitor-only.
- `obsidian-npm-audit` runs `npm --prefix .obsidian/plugins/criv audit
  --audit-level=high` and fails the check for a high or critical advisory, or
  when npm cannot obtain advisory data.
- `vscode-npm-audit` applies the same blocking policy to
  `extensions/vscode-criv`.

These steps have no globs, so they execute for every `hk check --all` run. CI
continues to invoke the standard `mise run check` entry point; it needs no
parallel audit command.

## Consequences

The hosted npm audit service becomes a required availability dependency for the
full check. This is deliberate: an unavailable advisory feed must not appear as
a clean audit.

Rust findings remain visible in both local and CI output, but do not block
contributions until a reproducible Rust advisory-database update policy is
separately decided. This preserves the monitoring conclusion in
[[dependency-evaluations]].

Contributors can run a single audit step through `hk check --all --step
<name>`; `hk check --all --plan` shows all three. The existing `cargo-audit`
mise tool pin remains required for the Rust monitor.

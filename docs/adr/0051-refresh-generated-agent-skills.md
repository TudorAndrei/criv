---
id: ADR-0051
kind: decision
title: Refresh Generated Agent Skills Explicitly
status: accepted
date: 2026-07-30
supersedes:
  - ADR-0010
governs:
  - src/init.rs
---

# Refresh Generated Agent Skills Explicitly

## Context

ADR-0010 made `criv init` create skill files only when absent, while also
directing future skill changes to ship through that same initializer. Those
rules conflict: corrected templates could reach new vaults but not existing
ones. The generated skills are committed, so silently replacing them during an
ordinary initialization would create surprising working-tree changes.

## Decision

Skill files installed by criv are criv-owned generated artifacts; users do not
edit them. Each installed skill carries a hash of its embedded template.
`criv check` reports stale or legacy marked skills in text output without
changing diagnostics or its exit status. Users refresh them explicitly with
`criv init --force-skills`, which retains the initializer's confined,
symlink-safe write path.

## Consequences

Existing vaults get an actionable notice whenever a skill template changes,
but ordinary `criv init` remains create-only. Refreshing deliberately replaces
any local skill edits, consistent with their generated-artifact status. JSON
and GitHub check output stay machine-readable and unchanged.

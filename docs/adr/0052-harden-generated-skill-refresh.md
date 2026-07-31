---
id: ADR-0052
kind: decision
title: Harden Generated Skill Refresh
status: accepted
date: 2026-07-31
supersedes:
  - ADR-0051
governs:
  - src/init.rs
  - src/init/templates.rs
  - src/check.rs
---

# Harden Generated Skill Refresh

## Context

ADR-0051 established explicit refreshes for criv-owned skills, but its initial
implementation allowed advisory detection to affect non-text checks and made
the refresh command run unrelated initializer scaffolding. The marker handling
also needed to tolerate an accidentally marked shipped template.

## Decision

Stale-skill detection is text-output-only and best-effort: unreadable skill
files are ignored rather than changing diagnostics, machine output, or exit
status. `criv init --force-skills` is a skills-only operation; it refreshes
only the twelve criv-owned destinations and does not add hooks, editor files,
configuration, or ignore rules.

Template stamping canonicalizes away an existing criv marker before hashing and
writing one. This keeps a mistakenly marked embedded template refreshable while
assets remain marker-free by convention.

## Consequences

The refresh command is safe to recommend from `criv check` even for vaults
whose users declined optional integrations. A stale-skill note remains
strictly advisory, and all machine-readable check formats retain their previous
contract.

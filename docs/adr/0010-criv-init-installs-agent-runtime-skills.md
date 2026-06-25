---
id: ADR-0010
kind: decision
title: Criv Init Installs Agent Runtime Skills
status: accepted
date: 2026-05-13
governs:
  - src/init.rs
---

# Criv Init Installs Agent Runtime Skills

## Context

The original spec described skill files as vault documentation under `docs/`.
During dogfooding on May 13, the runtime expectation was clarified: generic
agent skills belong under `.agents/skills`, and Claude Code skills belong under
`.claude/skills`. The user explicitly rejected direct manual creation and asked
that criv itself create those files in the appropriate spaces.

## Decision

Extend `criv init` in `src/init.rs` so the tool idempotently creates runtime
skill directories and `SKILL.md` files under both `.agents/skills` and
`.claude/skills`, in addition to the normal vault documentation under `docs/`.

The generated runtime skills cover criv workflow, criv-backed decision
development, writing decisions, referencing code, and checking drift.

## Consequences

New criv vaults can be used by agent runtimes without a separate manual skill
installation step. Existing vaults can rerun `criv init` safely because the
initializer only creates missing files and does not overwrite user edits.

This also means future skill changes should be made in the initializer template
and then generated through `criv init`, rather than being treated as one-off
local files.

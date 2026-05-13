---
id: ADR-0011
kind: decision
title: Embed Runtime Skill Templates as Assets
status: accepted
date: 2026-05-13
governs:
  - src/init.rs
---

# Embed Runtime Skill Templates as Assets

## Context

[[ADR-0010]] established that `criv init` creates agent runtime skills under
`.agents/skills` and `.claude/skills`, but it also kept duplicate skill notes
under `docs/`. That made vault documentation and runtime skill installation
share the same content surface even though the runtime skills already exist as
agent source files in this repository.

Accepted ADRs are append-only records. This ADR records the follow-up decision
instead of changing [[ADR-0010]].

## Decision

Move runtime skill template content into source assets under
`assets/skills/**/SKILL.md` and embed those markdown files into the criv binary
with `include_str!` in [[src/init.rs]].

`criv init` continues to install the embedded skill templates into
`.agents/skills` and `.claude/skills`, but it no longer creates duplicate skill
notes under `docs/`.

## Consequences

The `docs/` folder stays focused on vault documentation and ADRs. Runtime skill
content has a single source template location that can be embedded into release
binaries and installed idempotently into new vaults.

Future skill content changes should be made in `assets/skills/**/SKILL.md`.

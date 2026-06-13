---
id: ADR-0021
kind: decision
title: Audit Remediation Boundaries
status: accepted
date: 2026-06-13
governs:
  - src/config.rs
  - src/init/templates.rs
  - src/watch.rs
  - hk.pkl
  - mise.toml
  - README.md
---

# Audit Remediation Boundaries

## Context

The 2026-06-11 improve audit found three areas where criv accepted user-facing
surface area without enough behavior or validation: the Obsidian plugin was
tracked and generated but not part of repository-level checks, several
`criv.toml` knobs were parsed or emitted without meaningful behavior, and
`criv watch --port` accepted a status-port value while exposing no endpoint.

[[0009-obsidian-plugin-as-state-consumer|ADR-0009]] already established the
plugin as a local state consumer installed by `criv init`.
[[0001-local-cli-vault-architecture|ADR-0001]] makes the CLI and local state
files criv's public surface. [[0006-fff-source-index-and-incremental-watch|ADR-0006]]
keeps watch mode focused on source indexing and local state refresh.

## Decision

Treat the Obsidian plugin as a maintained criv product surface, not a disposable
example. Because `criv init` installs it by default and the repository tracks
its source and generated artifacts, plugin dependencies and build tools must be
pinned, and repository-level checks must cover plugin linting, tests, and
builds.

Keep generated `criv.toml` small and behavior-backed. Remove `source.languages`
because a repository can be multi-language and source parsing should be inferred
from supported file types. Remove `index.notes` because there is no clear
note-index backend behind that setting. Remove `[obsidian].plugin` because
plugin installation is an `criv init` scaffolding option, not persistent vault
configuration.

Keep `index.source` and make it effective. A docs-only vault should be able to
disable source collection, source indexing, source graph construction, and
generated source-index state through `index.source = false`.

Remove `criv watch --port` instead of implementing a status endpoint. A status
service would be a new runtime API surface and should require a separate
decision before it is added.

## Consequences

Repository validation gets slower because it includes plugin checks, but the
checked surface matches what `criv init` ships.

Unknown TOML keys remain ignored by serde's default behavior, but removed knobs
are no longer first-class `Config` fields and should not appear in generated
vault config.

User-facing CLI flags and config fields should correspond to active behavior.
Future placeholder config or runtime service flags should be deferred until the
behavior exists and has a clear owner.

---
id: ADR-0147
kind: decision
title: Validate LikeC4 Capabilities Not Runtime Versions
status: accepted
date: 2026-10-30
governs:
  - src/c4/likec4.rs
  - crates/criv-wasm/src/likec4.rs
  - packages/criv-editor-state/src/wasmHost.ts
  - assets/likec4-contract.json
  - package.json
  - mise.toml
---

# Validate LikeC4 Capabilities Not Runtime Versions

## Context

The exact runtime checks introduced by
[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] and retained by
[[0100-agent-authored-language-independent-c4-architecture|ADR-0100]] reject
working Node.js and LikeC4 installations after dependency updates. An installed
criv release cannot control a consuming repository's runtime versions. Exact
checks break pre-commit hooks and CI even when the bridge API still works.

## Decision

Refine those decisions' runtime compatibility clauses: do not reject Node.js
or LikeC4 based on version strings. Continue resolving Node.js from `PATH` and
LikeC4 from local `node_modules`, without downloading either dependency.

Keep exact development pins in mise and the npm lockfile for reproducible
builds. Versions in the embedded contract are development recommendations,
not runtime requirements. Do not impose a root npm Node.js engine requirement.
Missing dependencies and bridge execution failures may suggest those versions.

Retain bridge protocol and revision validation, process limits, model
validation, and source-link validation. Actual API incompatibilities and
invalid architecture source remain errors; do not silently skip validation.
The Wasm projection validates the protocol and model shape, not the LikeC4
package version, and preserves the producer's reported version.

## Consequences

A dependency version difference alone cannot fail a hook, CI check, or editor
State load. This does not promise support for every Node.js or LikeC4 release:
incompatible APIs or model shapes can still fail with actionable diagnostics.
Older installed criv binaries retain their exact checks until upgraded.

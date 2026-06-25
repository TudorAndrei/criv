---
id: ADR-0001
kind: decision
title: Local CLI Vault Architecture
status: accepted
date: 2026-05-10
governs:
  - src/lib.rs
  - src/main.rs
  - src/init.rs
  - src/config.rs
---

# Local CLI Vault Architecture

## Context

The initial criv specification defined criv as a single-binary, local-only tool
that turns a repository's `docs/` directory into a validated graph of source
code, editorial documentation, and architectural decisions. The implementation
needed one authoritative programmatic surface for humans and agents, while
keeping hosted services, telemetry, and runtime APIs out of scope.

## Decision

Keep criv as a Rust CLI whose commands are the public interface. A criv vault is
rooted by `criv.toml`, `docs/`, `docs/adr/`, and local `.criv/` state. The CLI
owns initialization, validation, query, search, watch, and enforcement behavior
through `src/lib.rs`, `src/main.rs`, `src/init.rs`, and `src/config.rs`.

Obsidian integration is a consumer of local state, not an alternate backend or
service API. Agents interact with criv by reading skill files, editing docs and
ADRs, and shelling out to the CLI.

## Consequences

This keeps the implementation easy to run in any repository and makes every
automation path reproducible from shell commands. It also means criv should be
careful about adding long-running service assumptions or network-dependent
features.

Future API additions should first justify why they cannot be expressed through
the existing CLI and state files.

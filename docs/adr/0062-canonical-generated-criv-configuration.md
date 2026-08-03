---
id: ADR-0062
kind: decision
title: Canonical Generated criv Configuration
status: accepted
date: 2026-08-03
governs:
  - .taplo.toml
  - criv.toml
  - src/init/**
---

# Canonical Generated criv Configuration

## Context

`criv init` serialized its default configuration with
`toml::to_string_pretty`. That serializer expands short arrays with four-space
indentation, while the repository's pinned Taplo formatter collapses those
arrays and uses two-space indentation. A newly initialized vault therefore
started with formatter drift that the first automatic pre-commit hook rewrote.

The semantic defaults still need a typed representation so tests detect drift
between the intended configuration and its emitted form. A general configurable
TOML serializer is unnecessary for one stable scaffold.

## Decision

Ship the default `criv.toml` as a checked-in canonical fixture and embed it in
the initializer. Keep the typed `DefaultConfig` model as the semantic oracle;
tests parse the fixture and compare it with that typed value.

Declare the repository's canonical TOML width and indentation in
`.taplo.toml`. The generated fixture must be byte-identical to initialization
output and a no-op under that formatter configuration.

## Consequences

New vaults no longer acquire a formatting-only `criv.toml` change at their
first commit. Template changes require updating the typed defaults and fixture
together, which makes semantic and formatting drift explicit in tests.

The emitted representation is intentionally maintained as a template rather
than delegated to the TOML serializer. This is a narrow maintenance cost in
exchange for deterministic bytes across serializer and formatter upgrades.

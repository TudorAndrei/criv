---
id: ADR-0001
kind: decision
title: Elixir source contract
status: accepted
date: 2026-08-20
governs:
  - lib/**/*.ex
  - test/**/*.exs
policy:
  patterns:
    - id: no-system-command
      language: elixir
      pattern: "System.cmd($$$ARGS)"
      message: Do not start operating-system commands from this fixture.
---

# Elixir source contract

## Context

The acceptance fixture exercises Elixir source indexing.

## Decision

Use first-class Elixir source identities and the inline policy above.

## Consequences

Operating-system commands fail the fixture's policy check.

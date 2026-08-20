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

## Elixir source contract

The acceptance fixture uses first-class Elixir source identities.

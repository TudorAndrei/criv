---
id: ADR-0001
kind: decision
title: Avoid println in governed Rust
status: accepted
date: 2026-08-03
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: println!($$$ARGS)
---

# Avoid println in governed Rust

## Context

The fixture needs one deterministic structural policy match.

## Decision

Govern Rust output calls with an inline pattern.

## Consequences

Incremental refreshes must retain or rescan the match correctly.

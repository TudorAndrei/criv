---
id: ADR-0003
kind: decision
title: Adopt Proven Foundation Crates
status: accepted
date: 2026-05-12
governs:
  - src/lib.rs
  - src/config.rs
  - src/util.rs
  - src/vault.rs
  - src/state.rs
  - src/watch.rs
---

# Adopt Proven Foundation Crates

## Context

The May 12 dependency review found several places where criv was carrying
hand-rolled infrastructure: CLI parsing, error handling, glob matching, markdown
event scanning, binary detection, MIME hints, YAML frontmatter parsing, snapshot
hashing, and raw watcher events.

## Decision

Use focused crates for foundational behavior before growing heavier backends:
`clap` for CLI parsing, `thiserror` for errors, `globset` for glob matching,
`pulldown-cmark` for markdown event parsing, `content_inspector` for text/binary
classification, `mime_guess` for extension MIME hints, `serde_norway` for YAML
frontmatter, `blake3` for stable hashes, and `notify-debouncer-mini` for
watcher debouncing.

The relevant implementation surfaces are `src/lib.rs`, `src/config.rs`,
`src/util.rs`, `src/vault.rs`, `src/state.rs`, and `src/watch.rs`.

## Consequences

This reduces custom parsing and matching code while improving compatibility with
real markdown, glob, and filesystem behavior. Snapshot hashes become stable
content addresses instead of process-local hash output.

Some crates are intentionally deferred. `miette`, `infer`, `serde_yaml_ng`, and
`camino` remain documented evaluations until their benefits exceed the migration
cost.

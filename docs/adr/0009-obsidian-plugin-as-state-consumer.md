---
id: ADR-0009
kind: decision
title: Obsidian Plugin As State Consumer
status: accepted
date: 2026-05-13
governs:
  - src/init.rs
---

# Obsidian Plugin As State Consumer

## Context

The specification includes an Obsidian plugin for source-reference previews,
pattern rendering, drift indicators, autocomplete, and plugin-side Rust helper
logic. The plugin should improve vault ergonomics without becoming a second
implementation of criv's graph logic.

## Decision

Ship an Obsidian sample-plugin-style scaffold from [[src/init.rs]]. The plugin
reads `.criv/state.json`, validates schema version, renders state-derived source
and pattern context, and delegates small shared helper logic to the WASM crate.

The CLI and plugin share link-resolution fixture cases. New vaults initialized
by criv receive the same fixture data in the generated plugin scaffold.

## Consequences

Obsidian remains a local UI over criv state. Source editing and authoritative
validation stay in the CLI.

Generated plugin artifacts must be kept reproducible through the plugin build
tooling. Release checks should include the plugin build when plugin templates or
WASM helper behavior changes.

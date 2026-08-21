---
id: ADR-0133
kind: decision
title: Use Exact Lowercase Documentation Asset Extensions
status: accepted
date: 2026-08-21
supersedes:
  - ADR-0131
governs:
  - Cargo.toml
  - src/discovery/mod.rs
  - src/vault.rs
  - src/state.rs
  - crates/criv-state-wire/src/lib.rs
  - crates/criv-wasm/src/**/*.rs
  - .obsidian/plugins/criv/src/**
  - extensions/vscode-criv/src/**
---

# Use Exact Lowercase Documentation Asset Extensions

## Context

[[0131-publish-verified-documentation-assets-for-native-previews|ADR-0131]]
adds the verified documentation asset inventory. Its implementation accepted
uppercase asset extensions, but Markdown and LikeC4 discovery accept exact
lowercase extensions only.

One discovery profile must not use different case rules for its file types.
The extension rule is also part of the native and Wasm validation boundary.

## Decision

Retain the complete asset inventory, signature verification, passive format
set, size bounds, State contract, editor behavior, tests, and all other
behavior from ADR-0131.

Accept documentation assets only when their extension is one of these exact
lowercase values: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, or `.pdf`.
Uppercase and mixed-case extensions are unsupported and do not enter the
inventory. Apply the same exact rule during native discovery, native MIME
validation, and Wasm projection validation.

Keep lexical processing and stop adding entries when the total size bound
would be exceeded. A later candidate does not enter the inventory after the
first total-size overflow.

## Consequences

Markdown, LikeC4, and documentation asset discovery use one case rule. Native
and Wasm consumers reject the same extension forms. Repositories that use an
uppercase asset extension must rename the file to a supported lowercase
extension before it can appear in State.

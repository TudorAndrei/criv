---
id: ADR-0075
kind: decision
title: LikeC4 Preview As The Default C4 Editor
status: accepted
date: 2026-08-04
governs:
  - assets/likec4-bridge.mjs
  - packages/criv-likec4/**
  - extensions/vscode-criv/**
---

# LikeC4 Preview As The Default C4 Editor

## Context

[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] makes agents
the LikeC4 DSL authors and makes the packaged LikeC4 renderer the user-facing
architecture view. The first VS Code adapter still opens a text editor and a
second preview beside it. This uses two editor groups for one artifact and puts
the agent-facing DSL in the primary position.

The preview also selects the first view in the normalized state. Views are
sorted for deterministic state, so the first item does not identify the view
owned by the opened file. LikeC4 layout data has a public `sourcePath` field for
each authored view. The host can use this field without parsing the DSL.

## Decision

Register a read-only custom text editor for `*.c4` with default priority. It
renders the complete validated LikeC4 workspace from `.criv/state.json` in the
same editor tab. Keep the normal text editor available through **Reopen Editor
With** for agents and maintainers who must edit the DSL.

Publish each authored view's LikeC4 `sourcePath` in the normalized view record.
When a `.c4` file opens, select the first deterministic named view whose source
path matches that file. Keep the view selector available when the file owns
more than one view. If no view matches, use the normal renderer fallback.

Do not parse LikeC4 source in the VS Code extension. Keep the explicit preview
command for a maintainer who opens a `.c4` file as text and wants a second
preview group.

## Consequences

Users open architecture files directly into the rendered view. A System
Context, Container, or Component source file opens its own named view instead
of an unrelated alphabetically-first view. Editing remains an explicit action.

The normalized view record gains optional source ownership metadata. Generated
or implicit views without a source path still render through the normal
fallback. The webview remains a projection of validated state and does not
become a LikeC4 parser or validator.

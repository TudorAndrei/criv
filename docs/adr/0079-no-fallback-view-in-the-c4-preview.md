---
id: ADR-0079
kind: decision
title: No Fallback View In The C4 Preview
status: accepted
date: 2026-08-04
supersedes:
  - ADR-0078
governs:
  - extensions/vscode-criv/src/c4Preview.ts
  - extensions/vscode-criv/src/c4PreviewModel.ts
  - packages/criv-likec4/src/protocol.ts
  - packages/criv-likec4/src/renderer.ts
---

# No Fallback View In The C4 Preview

## Context

[[0078-the-editor-follows-preview-navigation|ADR-0078]] made the editor follow
renderer navigation, and kept the earlier rule that a file owning no named view
opens at a fallback view.

[[0077-c4-standard-alignment-for-the-likec4-workspace|ADR-0077]] moved every
named view into `views/`. The fallback then drew one overview for eight model
files that own no view. Two files rendered the same diagram with no way to tell
which one owned it, and a reader could not tell an empty file from a real view.

## Decision

A preview renders the view its file owns, and nothing else. There is no
fallback view at any layer:

- `preferredLikeC4ViewId` returns nothing for a file that owns no view, and the
  extension renders a status message that names the file and points at
  `views/`.
- `CrivLikeC4Renderer.replace` sets no view when the requested id is absent,
  rather than substituting one.
- `defaultLikeC4ViewId` is removed from the shared protocol, so no consumer can
  reintroduce the substitution.

The editor still follows renderer navigation, as ADR-0078 decided. When the
webview reports `selectView`, the extension resolves the file that owns the
target view and opens it with the preview in the same editor group, so the tab,
the breadcrumb, and the diagram name the same view. `CrivLikeC4Model` carries
the LikeC4 workspace root so a consumer can join it to a view's `sourcePath`.

## Consequences

An open preview always answers one question: which view does this file own. A
model file states plainly that it owns none, which is the truth about a file
that declares elements and relationships.

Reaching a diagram from a model file now takes a navigation step through a view
file, rather than an overview the model file never owned.

The Obsidian plugin inherits the same rule through the shared renderer: a note
or source file with no view renders the empty state instead of a substituted
diagram.

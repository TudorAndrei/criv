---
id: ADR-0078
kind: decision
title: The Editor Follows Preview Navigation
status: accepted
date: 2026-08-04
governs:
  - extensions/vscode-criv/src/c4Preview.ts
  - extensions/vscode-criv/src/c4PreviewModel.ts
  - packages/criv-likec4/src/protocol.ts
---

# The Editor Follows Preview Navigation

## Context

[[0075-likec4-preview-as-the-default-c4-editor|ADR-0075]] made the validated
preview the default `.c4` editor.
[[0076-focused-likec4-workspace-navigation|ADR-0076]] gave each view file one
primary named view and had VS Code remember the selected view across state
refreshes.

The preview is a custom text editor bound to one document. A `selectView`
message only recorded the view id, so navigating inside the diagram left the
open tab on the file the reader started from. After
[[0077-c4-standard-alignment-for-the-likec4-workspace|ADR-0077]] moved every
view into `views/`, a reader who opened a model file or `specification.c4` saw
the fallback overview and had no way to reach the owning file from the diagram.

The state envelope already publishes the LikeC4 workspace root, and each view
already publishes its `sourcePath`.

## Decision

The editor follows renderer navigation. When the webview reports `selectView`,
the extension resolves the file that owns the target view and opens it with the
preview in the same editor group. The tab, the breadcrumb, and the diagram then
name the same view.

`CrivLikeC4Model` carries the workspace root, so a consumer can turn a view's
`sourcePath` into a workspace file. `likeC4ViewDocumentPath` in the shared
protocol performs that join, and `c4NavigationTarget` returns nothing when the
open document already owns the view, when the view owns no file, or when the
workspace root is absent.

A file that owns no view still opens at the fallback overview. Navigation is
the way out of that fallback.

## Consequences

Reading a diagram and reading its source stay in step, and the reader can edit
the view they are looking at without searching the tree for its file.

Each navigation replaces the active editor rather than adding a tab, so a
reading path does not accumulate tabs. The back control in the renderer moves
the diagram, and the following editor open moves the tab with it.

The Obsidian plugin keeps the earlier behaviour: it selects the view owned by
the opened file and does not follow navigation with a leaf change.

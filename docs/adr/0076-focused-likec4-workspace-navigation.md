---
id: ADR-0076
kind: decision
title: Focused LikeC4 Workspace Navigation
status: accepted
date: 2026-08-04
governs:
  - packages/criv-likec4/**
  - extensions/vscode-criv/**
  - .obsidian/plugins/criv/**
  - .vscode/**
---

# Focused LikeC4 Workspace Navigation

## Context

[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] established
one LikeC4 workspace and named views. [[0075-likec4-preview-as-the-default-c4-editor|ADR-0075]]
made the validated preview the default VS Code editor. The first authored
workspace kept each C4 level in one file and used broad relationship
predicates. Those predicates pulled detailed elements into overview and Code
views. The result was one large graph instead of a useful C4 drill-down path.

LikeC4 merges source files below one project root. Scoped views become the
default navigation target for their element. The React renderer publishes
navigation events through `onNavigateTo`. The official LikeC4 VS Code
extension supplies DSL language services but does not register a competing
custom editor.

## Decision

Keep one LikeC4 project in `docs/architecture/`. Split specifications, model
elements, relationships, and views into folders. Keep one primary named view
in each view file. Use title paths to group views under Overview, Components,
and Code.

Use scoped views for the System Context to Container to Component path. Use
explicit `navigateTo` targets when a component has a selected Code view. Do not
use broad `include * -> *` predicates. A view includes its selected elements
and the relationships that LikeC4 derives between them.

The shared renderer handles `onNavigateTo`, selects the target view, shows
navigation history controls, and notifies the host of the new selection. VS
Code remembers the selected view across state refreshes. Obsidian selects the
view owned by the opened source file. Both host selectors follow renderer
navigation.

Recommend `likec4.likec4-vscode` for this repository and associate `.c4` text
documents with its `likec4` language. Keep the official extension optional.
Criv remains the default custom preview, validation authority, state owner, and
source-link provider. Criv language features accept both `likec4` and the
standalone `criv-c4` language ID.

## Consequences

Each file opens a focused diagram. Users can move from the system view to a
container, then to its components and selected Code view without leaving the
preview. The workspace model still has one stable identity for each element.

The official extension supplies syntax highlighting, completion, formatting,
and its language server when installed. It does not become a required runtime
dependency and does not replace criv validation or the default preview.

---
id: ADR-0080
kind: decision
title: Co-locate Primary LikeC4 Views With Their Models
status: accepted
date: 2026-08-04
supersedes:
  - ADR-0077
  - ADR-0079
governs:
  - .agents/skills/c4-authoring/SKILL.md
  - assets/skills/c4-authoring/SKILL.md
  - extensions/vscode-criv/src/c4/preview.ts
  - extensions/vscode-criv/src/c4/previewModel.ts
  - packages/criv-likec4/src/protocol.ts
  - packages/criv-likec4/src/renderer.ts
---

# Co-locate Primary LikeC4 Views With Their Models

## Context

[[0077-c4-standard-alignment-for-the-likec4-workspace|ADR-0077]] put every
model declaration and every named view in separate files. This made ownership
clear, but it also made each domain model file an empty preview.

[[0079-no-fallback-view-in-the-c4-preview|ADR-0079]] correctly removed the
arbitrary fallback diagram. Its status message then sent every reader to
`views/`, because ADR-0077 required that folder to own every view.

LikeC4 already removes model duplication. It recursively loads the workspace
and merges its source blocks into one model. A view selects shared model
elements and derived relationships; it does not copy their declarations.
LikeC4 also supports scoped views, inherited views, and global predicate and
style groups. A strict model-file and view-file split is not required for any
of these properties.

## Decision

Keep one LikeC4 project under `docs/architecture/`. Declare each architecture
element and relationship once in the merged model.

A domain file may contain its model declarations and the primary named views
that explain that domain. Prefer one primary view. A large Code domain may own
more than one focused view when each view answers a different module question.
Keep a separate view file for a cross-domain workflow or another view that has
no single model owner.

Use the LikeC4 view `sourcePath` as the preview ownership contract. Opening a
domain file selects a named view declared in that file. A shared source such as
`specification.c4` can own no view and must show an explicit empty state.

Keep the no-fallback rule from ADR-0079. The renderer does not select an
unowned diagram. Change the VS Code empty-state message so that it points to an
architecture file that declares a named view, not specifically to `views/`.
Renderer navigation continues to open the file that owns the target view.

Put repeated view styles and selections in global style and predicate groups.
Use view inheritance only when the derived view is a more detailed form of its
base view.

Retain the C4 rules from ADR-0077:

- Declare each element once and at one C4 level.
- Tag every external person and system with `external` and grey it in views.
- Keep the project repository as an external software system.
- Keep published state as a data-store container inside criv.
- Keep the shared renderer as one criv container used by both editor adapters.
- Put process hosting in Deployment models, not Container relationships.
- Keep a hand-authored Code model as a true roll-up of its Component model.
- Keep cross-cutting helpers, re-export barrels, and bundler shims outside Code
  architecture, and name them in a comment.
- Start relationship labels with a capital present-tense verb, and add a
  technology when a relationship crosses a process, language, or storage
  boundary.
- Keep at least one Dynamic refresh view and one developer-workstation
  Deployment view.

## Consequences

A reader can open `cli.c4`, `vscode.c4`, or another domain file and see the
diagram that explains the model in that file. An agent can edit the model and
its primary view together without duplicating elements or relationships.

Cross-domain workflow views remain separate because no one domain owns them.
Shared specification files can still show an empty state, but this state is
truthful and does not block navigation to domain views.

The workspace has fewer files and fewer path-only navigation steps. A domain
file can become larger because it now contains both architecture facts and a
small view projection.

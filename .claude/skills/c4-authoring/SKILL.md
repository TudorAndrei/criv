---
name: c4-authoring
description: Use when authoring or reviewing criv C4 architecture artifacts, including Mermaid C4 blocks, standalone .c4 files, source anchors, and implementation drift expectations.
---

# C4 authoring

Use this skill when creating or reviewing C4 architecture artifacts for a criv
vault.

## Source Of Truth

C4 artifacts are text/code first. An LLM should generate text, a human should
review the text, and `criv check` should validate it before implementation work
depends on it. Rendered diagrams in Obsidian, VS Code, Mermaid, Merman, or other
viewers are projections over the source text.

Standalone `.c4` files are a filetype convention, not a custom DSL. The file
contains Mermaid C4 or DOT directly.

## Levels

Keep each diagram at one C4 level:

- System Context: people, the system in scope, and external systems.
- Container: deployable or runnable parts inside one software system.
- Component: major responsibilities inside one container.
- Code: code elements that implement one component or a generated source graph.

Use Code diagrams sparingly for focused implementation stories. Do not turn a
whole application into a hand-authored class diagram unless the artifact is an
explicit generated source graph.

## .c4 Files

Infer format from the first meaningful non-comment line:

- `C4Context`, `C4Container`, or `C4Component` means Mermaid C4.
- `digraph`, `graph`, `strict digraph`, or `strict graph` means DOT.

Use filename-derived levels:

- `context.c4` or `01-context.c4`
- `container.c4` or `02-container.c4`
- `component.c4`, `components.c4`, or `03-components.c4`
- `code.c4` or `04-code.c4`

Do not add extra required metadata lines for information that the file already
communicates. `criv:format` is optional and only asserts the inferred format.

## Mermaid C4 Rules

Every element should have:

- a stable alias;
- a readable name;
- a short responsibility;
- technology when the level is Container or Component and the technology is
  known.

Every relationship should have a label with a meaningful verb. Prefer small,
readable diagrams over large mixed-abstraction canvases.

## Source Anchors

Use `criv:source` anchors when an element maps to implementation.

Prefer stable interface-bearing anchors:

- public or exported functions and methods;
- structs, enums, classes, and interfaces;
- modules, components, or files that represent a real architectural boundary.

Avoid anchoring high-level C4 elements to private helper internals unless the
diagram is a narrow Code-level view. Body-only refactors should not require a
C4 update when the interface is unchanged, but input, output, field, variant, or
exported member changes should trigger a diagram review.

## Review Checklist

Before finishing:

- The title states the diagram level and scope.
- The diagram uses one abstraction level.
- Elements have names and responsibilities.
- Relationships are directional and labelled.
- Source anchors point at real implementation symbols when the diagram claims to
  describe existing code.
- The artifact remains useful as text without opening a visual renderer.

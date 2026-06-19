---
id: c4-rendering-options
kind: doc
title: C4 Rendering Options
---

# C4 Rendering Options

This note records candidate libraries and formats for visualizing criv C4
diagrams beyond the current Mermaid and DOT split decided by
[[0026-mermaid-c4-diagrams-as-vault-content|ADR-0026]] and
[[0030-dot-for-generated-code-architecture|ADR-0030]].

## Non-Negotiable Constraint

C4 architecture artifacts must be code/text first, as decided by
[[0031-text-first-c4-architecture-formats|ADR-0031]].

The source of truth must be a plain-text or structured-text artifact that an LLM
can generate, a human can review in a diff, and `criv check` can validate before
implementation work depends on it. Visual renderings, editor canvases, whiteboard
views, and browser viewers are projections over the text source. They are not
the authoritative architecture record.

## Current Baseline

- Mermaid remains the best fit for authored System Context, Container, and
  Component diagrams because it renders in Markdown-oriented tools and criv can
  parse a narrow C4 subset without adding a renderer dependency.
- Graphviz DOT remains the best fit for the exhaustive generated Code diagram
  because the graph can contain hundreds of code nodes and call edges.
- C4 Code diagrams are optional and normally tooling-generated. The C4 model
  describes Level 4 as zooming into a single component with UML class diagrams,
  ERDs, or similar tooling-generated views.

## Candidate List

Under the text-first constraint, candidates split into two groups:

- source formats that criv may generate, parse, normalize, and validate;
- viewer libraries that editor integrations may use to render or explore the
  source format.

Viewer libraries are not source formats. They can improve review ergonomics,
but they do not satisfy ADR-0031 unless their state is derived from a
reviewable text artifact that criv can verify.

### Source Formats

| Candidate | Role | Strengths | Acceptance gate |
| --- | --- | --- | --- |
| Mermaid C4 | Current authored Context, Container, and Component source format | Markdown-native, LLM-friendly, easy to diff, already parsed by criv. | Keep the parsed subset narrow and validate all supported C4 constructs through [[src/c4.rs]] and [[src/check.rs]]. |
| DOT | Current generated exhaustive Code architecture source format | Plain text, deterministic, better fit than Mermaid for the dense generated code graph. | Verify generated freshness and optionally parse enough DOT to reject malformed checked-in generated architecture. |
| `.c4` | Preferred criv-owned standalone artifact container | Keeps Mermaid C4 or DOT source directly in a file that editor viewers can claim by extension. | Treat `.c4` as a convention, not a DSL: infer the inner format from contents, derive C4 level from filename, and validate through criv before rendering. |
| Structurizr DSL/JSON | Strong candidate for model-first C4 interchange | Native C4 model concepts and export paths to several renderers. | criv must parse or import the DSL/JSON into its normalized C4 model; the Java CLI cannot be required for normal checks. |
| D2 | Candidate generated static text output | Readable declarative text with modern diagram tooling and Structurizr exporter support. | criv must own a C4-compatible subset and validation rules before treating D2 as a supported source format. |
| JSON Canvas | Candidate structured-text canvas source for Obsidian | JSON nodes, edges, groups, labels, and coordinates can be reviewed and generated. | Only acceptable if criv writes semantic C4 metadata and can validate node/edge references plus normalized C4 semantics. |
| Excalidraw JSON | Candidate structured-text whiteboard source | JSON scene files can represent review-friendly diagrams and hand-tuned layout. | Only acceptable if criv owns metadata conventions that make C4 semantics round-trip independently of presentation details. |
| C4-PlantUML | External text interchange option | Mature PlantUML macro set for C4 diagrams. | Do not require Java or Graphviz for `criv check`; support only through a criv-owned parser/import path or another normalized text format. |

### Viewer Libraries

| Candidate | Role | Strengths | Boundary |
| --- | --- | --- | --- |
| React Flow | Obsidian/VS Code interactive viewer | Good node/edge component model, controls, labels, minimaps, grouping, and editor interactions. | Reads criv-generated text/JSON or state; never becomes the architecture source of truth. |
| Cytoscape.js | Dense graph exploration viewer | Strong graph visualization and analysis features for source graphs and large relationship sets. | Useful for exploring generated architecture, but C4 semantics must come from criv-normalized data. |
| ELK.js | Browser-side layout engine | Useful automatic layout for nested/compound graphs before display in React Flow, Cytoscape.js, JSON Canvas, or Excalidraw. | Layout is projection metadata. It can be regenerated and must not be required for semantic validation. |
| Mermaid renderer | Markdown/editor preview | Familiar rendering path for existing Mermaid C4 blocks. | Rendering is optional; criv validates the Mermaid text, not the rendered image. |
| Merman | Browserless Mermaid renderer candidate | Rust implementation that may fit Obsidian/WASM packaging better than a browser automation renderer. | Renderer only. criv must not treat Merman render success as semantic validation or require it for normal CLI checks. |

## Merman Spike Result

Merman is a Rust crate family, not the npm package named `merman`. The crate
exposes a renderer API such as `HeadlessRenderer::render_svg_sync`, and it
targets Mermaid 11.15.0 parity, so it remains a good future candidate for a
Rust/WASM rendering path.

For the current Obsidian viewer, use bundled Mermaid.js instead. The Obsidian
plugin is already a TypeScript bundle, while adopting Merman would require a
new Rust/WASM packaging step for diagram rendering in addition to the existing
`criv-wasm` helper. That may still be worthwhile later, but it should not block
the text-first `.c4` workflow.

This decision does not change validation: `criv check` parses and verifies the
text source and does not depend on Mermaid.js, Merman, Node, Chromium, Java,
Graphviz, or a renderer.

## Recommendation

Keep Mermaid plus DOT as the default architecture for now.

Use `.c4` as the preferred criv-owned standalone source container for
architecture artifacts that are not ordinary prose notes. A `.c4` file should
contain Mermaid C4 or DOT directly. It should not introduce a criv-specific DSL,
and it should not require redundant directives for format or level when the
content and filename already provide them.

For a future model-first C4 direction, prototype Structurizr DSL/JSON first
because it is code/text first, preserves C4 semantics, and can export to several
downstream formats, including the formats criv already uses.

For generated text outputs and interactive projections, test separate paths:

- D2 as a lighter diagrams-as-code alternative to DOT for generated static
  architecture notes.
- JSON Canvas or Excalidraw JSON only when criv-owned metadata makes the saved
  artifact reviewable and validatable.
- ELK.js plus React Flow or Cytoscape.js only as editor viewer infrastructure,
  especially if the Obsidian plugin needs navigation, filtering, or source-aware
  graph exploration.
- Merman as the first Mermaid rendering candidate for Obsidian if it bundles
  cleanly; otherwise bundle Mermaid.js before looking for another renderer.

Treat JSON Canvas and Excalidraw as supported source formats only if their saved
JSON remains reviewable, deterministic enough for checks, and carries criv-owned
C4 metadata. Otherwise treat them as viewer/export projections.

## Viewer Boundary

criv does not need to embed a full diagram viewer in the core CLI binary.
Viewer code belongs in editor integrations: the current Obsidian plugin and a
future VS Code extension. Those viewers can use JavaScript libraries, browser
APIs, and WASM without making them part of criv's normal CLI runtime.

The pattern should match [[0009-obsidian-plugin-as-state-consumer|ADR-0009]]:
editor plugins are local UIs over criv state, while authoritative generation and
validation stay in the CLI. [[crates/criv-wasm/src/lib.rs]] already exposes a
small WASM helper for the Obsidian plugin. If diagram parsing, normalization, or
validation needs to run in editors, move that logic into a reusable Rust module
or crate and expose it through `criv-wasm` rather than reimplementing it in each
viewer.

## Output And Verification

criv should own two surfaces:

- deterministic architecture output from indexed repository data;
- verification of every C4 architecture format that criv claims to support,
  whether the file was authored by a human, generated by criv, or generated by
  another C4-capable tool.

That means each supported format needs a parser or validator that maps into a
normalized C4 model before checks run. The normalized model should preserve:

- diagram level and scope;
- elements with aliases, labels, descriptions, technologies, categories, and
  optional `criv:source` targets;
- relationships with endpoints, labels, and direction;
- boundaries or groups as notation metadata, not architecture elements;
- optional layout metadata for viewer/export formats such as JSON Canvas or
  Excalidraw.

After normalization, criv can reuse the existing validation ideas from
[[src/check.rs]]: duplicate aliases, invalid level/category combinations,
unresolved relationships, missing relationship labels, missing element metadata,
and stale source references.

| Format | criv output role | criv verification role |
| --- | --- | --- |
| Mermaid C4 | Keep as the authored Markdown-native format for Context, Container, and Component diagrams. | Already parsed from fenced blocks by [[src/c4.rs]] and validated by [[src/check.rs]]. |
| DOT | Keep as the generated exhaustive Code architecture format. | Verify generated freshness by regenerating expected output, and optionally parse DOT enough to catch malformed checked-in generated files. |
| `.c4` | Use as the preferred standalone extension for Mermaid C4 and generated DOT architecture artifacts. | Infer inner format from contents, infer C4 level from filename, include artifacts in state, and run the normalized C4/source checks. |
| JSON Canvas | Strong future Obsidian export target for editable architecture canvases. | Validate JSON schema shape, node/edge references, deterministic criv metadata, and normalized C4 semantics. |
| Excalidraw | Possible handoff/export target for review-friendly editable sketches. | Validate scene JSON, criv metadata, element references, and normalized C4 semantics. Treat presentation layout as non-authoritative. |
| Structurizr DSL/JSON | Good model-first interchange candidate if criv needs richer C4 semantics. | Prefer importing/exporting Structurizr JSON or a narrow DSL subset into the normalized model instead of depending on the Java CLI. |
| D2 | Possible generated static text output if it proves more readable than DOT. | Verify only a criv-owned D2 subset unless a Rust parser is adopted. Do not rely on the Go CLI for normal checks. |
| PlantUML/C4-PlantUML | External interchange/export option. | Verify only if criv implements a narrow parser or imports through another normalized format; do not make Java required for `criv check`. |

## WASM Viewer Recommendation

Use WASM for shared model logic, not for replacing CLI validation.

- The CLI remains the authority for `watch --once`, generated architecture
  files, `.criv/state.json`, and `criv check`.
- `criv-wasm` can expose state summaries, normalized C4 views, and validation
  diagnostics to Obsidian and future VS Code viewers.
- Editor viewers can use React Flow, Cytoscape.js, ELK.js, Mermaid rendering,
  JSON Canvas, or Excalidraw-specific UI code because those dependencies stay in
  the editor extension bundle.
- Generated architecture files should be deterministic enough that `criv check`
  can detect drift without launching an editor, browser, or renderer.

## Authoring Workflow

Use `.c4` artifacts as implementation inputs, not screenshots.

1. Generate or edit the Mermaid C4 or DOT text directly. For authored diagrams,
   use filename-derived levels such as `01-context.c4`, `02-container.c4`, and
   `03-component.c4`. For generated Code architecture, use `04-code.c4`.
2. Add `criv:source` comments only when an element maps to implementation.
   Prefer stable interface-bearing symbols so body-only refactors do not force
   architecture churn.
3. Run `criv watch --once` to refresh generated architecture and state.
4. Run `criv check` before relying on the diagram for implementation work.
5. Open the `.c4` file in Obsidian for the rendered projection. The visual view
   is for review ergonomics; the text and `criv check` remain authoritative.

## Sources

- [C4 model Code diagram](https://c4model.com/diagrams/code)
- [Structurizr DSL](https://docs.structurizr.com/dsl) and
  [Structurizr CLI export formats](https://docs.structurizr.com/cli/export)
- [Structurizr CLI installation](https://docs.structurizr.com/cli/installation)
- [C4-PlantUML](https://github.com/plantuml-stdlib/C4-PlantUML)
- [D2](https://d2lang.com/tour/intro/)
- [D2 install](https://d2lang.com/tour/install/) and
  [D2 Oracle API](https://d2lang.com/tour/api/)
- [JSON Canvas specification](https://jsoncanvas.org/spec/1.0/)
- [Excalidraw developer API](https://docs.excalidraw.com/docs/@excalidraw/excalidraw/api/props/excalidraw-api)
- [React Flow](https://reactflow.dev/learn)
- [Cytoscape.js](https://js.cytoscape.org/)
- [ELK.js](https://github.com/kieler/elkjs)
- [Mermaid usage](https://mermaid.js.org/config/usage.html) and
  [Mermaid CLI](https://github.com/mermaid-js/mermaid-cli)
- [Merman](https://github.com/latias94/merman)
- [PlantUML quick start](https://plantuml.com/starting) and
  [PlantUML Graphviz notes](https://plantuml.com/graphviz-dot)

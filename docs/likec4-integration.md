---
id: likec4-integration
kind: doc
title: LikeC4 Integration
targets:
  symbols:
    - src/c4.rs
    - src/c4/likec4.rs
    - assets/likec4-bridge.mjs
    - packages/criv-likec4/src/protocol.ts
    - packages/criv-likec4/src/renderer.ts
    - extensions/vscode-criv/src/c4/preview.ts
    - .obsidian/plugins/criv/src/main.ts
---

# LikeC4 Integration

[[0100-agent-authored-language-independent-c4-architecture|ADR-0100]] defines the
architecture contract. This document gives the implementation specification.

## Source and ownership

One vault has one LikeC4 workspace at `docs/architecture/`. All architecture
source files use the `.c4` extension. LikeC4 owns the language grammar, model
rules, layout, and visual output. The coding agent writes the source. It chooses
clear element names, responsibilities, relationships, and view titles. criv
does not create architecture source from source files, modules, imports, or
programming languages.

The workspace can contain agent-authored models at every useful C4 level. The
complete workspace describes one software architecture. Each view tells one
focused story at one level. A title names the level and the system, container,
component, or workflow in scope. A Code view is optional and zooms into one
important component. It does not group the repository by programming language.

This repository keeps agent-authored Code models and their focused views under
`docs/architecture/code/`. Each view explains one component or workflow instead
of copying the complete source index.

A Code model must stay a true roll-up of the component model. If
a module in component A imports a module in component B, the component model
must also show a relationship from A to B. Cross-cutting helper modules,
re-export barrels, and bundler shims stay outside the architecture; name them
in a comment at the top of the Code model file.

The workspace has one LikeC4 project. LikeC4 merges every source file into one
model. A domain file declares its elements and relationships once and also owns
the primary named views that explain that domain. A large Code domain can own
more than one focused view. Cross-domain runtime workflows stay in separate
view files. This ownership lets an editor select the correct preview from the
opened file path without copying model declarations.

```text
docs/architecture/
  specification.c4        element kinds, tags, and global style groups
  systems.c4              people, systems, and the System Context view
  cli.c4                  CLI components and their primary view
  interactions.c4         cross-container relationships and Container view
  deployment.c4           deployment nodes and Deployment views
  obsidian.c4             Obsidian components and their primary view
  vscode.c4               VS Code components and their primary view
  shared-renderer.c4      renderer components and their primary view
  state-projection.c4     WebAssembly components and their primary view
  code/                   selected component implementations and Code views
  views/dynamic/          cross-domain runtime sequences
```

Scoped views provide System Context to Container to Component navigation.
Selected containers and components use explicit `navigateTo` targets. View
title paths group diagrams under Overview, Components, Code, Dynamic, and
Deployment.

Two rules keep the levels readable. External people and systems carry the
`external` tag, and every view greys them, so the system boundary is visible.
A relationship label starts with a capital letter and a present-tense verb, and
does not end with a preposition; it carries a `technology` when it crosses a
process, a language, or a storage boundary.

Hosting belongs to the deployment model. A container diagram shows what a
container depends on. It does not show which application process contains it.

The source graph can show modules, symbols, calls, and imports as evidence for
the coding agent. These facts do not become C4 elements by themselves. The
agent selects the architecture boundary, gives each selected element a useful
name and responsibility, and links it to a stable source file or symbol.
External package imports do not create placeholder nodes.

## CLI validation flow

When the vault has at least one `.c4` file, `src/c4/likec4.rs` starts the embedded
`assets/likec4-bridge.mjs` program with the local Node.js command. The bridge
resolves the local `likec4` package. It does not use a global LikeC4 command and
does not download a package.

The exact runtime contract is:

| Item | Required value |
| --- | --- |
| Node.js | 26.5.1 |
| LikeC4 | 1.59.2 |
| React | 19.2.8 |
| React DOM | 19.2.8 |
| Bridge protocol | 1 |
| State schema | `criv.state.v1` |
| Process limit | 60 seconds |
| Standard output limit | 16 MiB |
| Standard error limit | 16 MiB |

The bridge uses `LikeC4.fromWorkspace`, `getErrors`, and `layoutedModel`. It
returns one JSON response with the exact runtime versions, revision, errors,
layout model, sorted elements, sorted relations, sorted views, and sorted source
links. Rust reads standard output and standard error at the same time. This
prevents a pipe deadlock when a large layout model fills an operating-system
pipe.

Rust rejects a timeout, too much output, invalid JSON, a protocol difference,
or any runtime version difference. LikeC4 errors use repository paths and
one-based lines in criv output. A vault with no `.c4` file does not need Node.js
or LikeC4.

## Source links

A LikeC4 element can have one criv source anchor:

```likec4
link ../../src/c4/likec4.rs 'source'
```

The `source` label is case-insensitive. The target is relative to the LikeC4
workspace. The bridge changes it to a repository-relative target. criv then
uses its normal source-target resolver. A missing file, line, symbol, or pattern
is an `invalid-likec4-source` error. Other LikeC4 links stay normal links.

## State and editor flow

`criv watch --once` writes the normalized layout model to the top-level
`architecture` field in `.criv/state.json`. The field has the bridge protocol,
LikeC4 version, workspace path, revision, raw layout model, elements,
relations, views, and source links.

The shared `packages/criv-likec4` package adapts this state to LikeC4 React. It
owns model replacement, stale-revision rejection, view selection, pan, zoom,
search, source-link events, disposal, and SVG export. SVG export puts the
LikeC4 shadow DOM in an SVG `foreignObject`, so the exported file contains the
same LikeC4 view and styles.

The Obsidian and VS Code packages are host adapters. They read criv state,
attach a monotonic host revision, select the host color scheme, and send source
link events to the host file API. Normalized view records contain LikeC4's
optional `sourcePath`. Both hosts use it to select the view owned by the opened
file without parsing `.c4` source. The shared renderer handles LikeC4
`onNavigateTo` events and reports the selected view to the host control. VS
Code remembers that selection across state refreshes.

There is no fallback diagram. A domain file opens the named views that it owns.
A shared file such as `specification.c4` can own no view and shows an explicit
status message. The status message points to another architecture file that
declares a named view; it does not assume that every view is under `views/`.

VS Code registers the read-only preview as the default `.c4` editor; **Reopen
Editor With → Text Editor** exposes the DSL. The repository recommends the
official `likec4.likec4-vscode` extension for DSL language services and maps
`.c4` text documents to its `likec4` language ID. That extension remains
optional and does not own the default preview. Neither host starts Node.js.
Their installed browser bundles contain the renderer and its assets. The VS
Code webview permits only its local resources under a strict content security
policy.

After an agent saves a `.c4` file, the normal watch task validates the complete
workspace and writes a new state model. Each host replaces the old model. A
response with an old revision cannot replace a new model. A closed view always
disposes its React root.

## Installation and CI

The root `package-lock.json` is the only npm lockfile. Use the pinned Node.js
version and run `npm ci`. Normal validation can then run without network
access. CI must run the Rust workspace tests, the two editor test suites,
`criv check`, and `npm audit`.

The 2026-08-04 implementation measurements used Node.js 26.5.1 on an Apple
arm64 development host and the repository model that existed during the test.
Architecture changes can change its element, view, and source-link counts:

| Measure | Result |
| --- | ---: |
| Node.js empty-process start | 0.02 s |
| Full `criv check` with LikeC4 layout | 3.80 s |
| Obsidian production JavaScript | 2,764,888 bytes |
| VS Code extension JavaScript | 26,311 bytes |
| VS Code LikeC4 webview JavaScript | 2,294,423 bytes |
| Cold browser bundle transfer | 17.5 ms |
| Cold state transfer | 2.9 ms |
| Cold first LikeC4 view | 112.2 ms |
| Test SVG export | 188,499 bytes |

The cold browser test used a local server and a new headless Chromium session.
The first-view point was the first LikeC4 React Flow node in the renderer shadow
root.

The lockfile license scan found no package without license data except the
three local workspaces. The shared workspace now declares MIT. Production
bundles keep esbuild legal comments. GPL and MPL packages in the lockfile are
development tools and are not in the production renderer bundles. The npm
audit after the lockfile update reports zero known vulnerabilities.

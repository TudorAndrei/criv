---
id: likec4-integration
kind: doc
title: LikeC4 Integration
targets:
  symbols:
    - src/likec4.rs
    - assets/likec4-bridge.mjs
    - packages/criv-likec4/src/protocol.ts
    - packages/criv-likec4/src/renderer.ts
    - src/c4_code.rs
    - extensions/vscode-criv/src/c4Preview.ts
    - .obsidian/plugins/criv/src/main.ts
---

# LikeC4 Integration

[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] defines the
architecture contract. This document gives the implementation specification.

## Source and ownership

One vault has one LikeC4 workspace at `docs/architecture/`. All architecture
source files use the `.c4` extension. LikeC4 owns the language grammar, model
rules, layout, and visual output. Agents write the source. criv does not read a
Mermaid C4 or DOT format and does not provide a migration command.

The workspace can contain manual system, container, and component models. criv
also writes `docs/architecture/04-code.c4`. The generated file contains only
language modules and import relations:

- Rust crates and `mod` declarations
- TypeScript and JavaScript modules and namespaces
- Python modules and packages
- Go packages

The generator does not make nodes for files, classes, functions, methods, or
calls. A file is only a source location for a module. Each language gets a
focused named view. There is no full Code view.

Module identity and nesting use these rules:

- Rust starts at the nearest `Cargo.toml` package. Hyphens become underscores.
  `lib.rs` and `main.rs` identify the crate, file-backed modules follow their
  path below `src/`, and nested `mod` declarations append their AST nesting.
  A `src/bin` target starts a separate crate identity.
- TypeScript and JavaScript use the repository-relative ES module path.
  Declared namespaces append their AST nesting to that module.
- Python strips the longest configured source root. A normal file is an
  importable module. An `__init__.py` file identifies its package.
- Go groups all files with the same directory and `package` declaration into
  one package node.

Private and public modules are both architecture nodes. Architecture describes
ownership, not only a public API. A repeated module or Go package has one node;
the first repository path in lexical order is its source anchor. Module ids are
stable hashes of the language and normalized module identity. Import edges are
present only when the imported module resolves to another generated node.
External package imports do not create placeholder nodes.

## CLI validation flow

When the vault has at least one `.c4` file, `src/likec4.rs` starts the embedded
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
link ../../src/likec4.rs 'source'
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
optional `sourcePath`. VS Code uses it to select the view owned by the opened
file without parsing `.c4` source. It registers the read-only preview as the
default `.c4` editor; **Reopen Editor With → Text Editor** exposes the DSL.
Neither host starts Node.js. Their installed browser bundles contain the
renderer and its assets. The VS Code webview permits only its local resources
under a strict content security policy.

After an agent saves a `.c4` file, the normal watch task validates the complete
workspace and writes a new state model. Each host replaces the old model. A
response with an old revision cannot replace a new model. A closed view always
disposes its React root.

## Installation and CI

The root `package-lock.json` is the only npm lockfile. Use the pinned Node.js
version and run `npm ci`. Normal validation can then run without network
access. CI must run the Rust workspace tests, the two editor test suites,
`criv check`, and `npm audit`.

The 2026-08-04 implementation measurements used Node.js 26.5.1 on an Apple arm64 development
host and the current repository model with 117 elements, seven views, and 113
source links:

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

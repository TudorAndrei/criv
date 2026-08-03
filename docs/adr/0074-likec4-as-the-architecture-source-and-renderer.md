---
id: ADR-0074
kind: decision
title: LikeC4 As The Architecture Source And Renderer
status: accepted
date: 2026-08-03
supersedes:
  - ADR-0026
  - ADR-0027
  - ADR-0028
  - ADR-0030
  - ADR-0032
governs:
  - src/c4.rs
  - src/c4_code.rs
  - src/likec4.rs
  - src/architecture.rs
  - src/check.rs
  - src/source_graph.rs
  - src/state.rs
  - src/vault.rs
  - crates/criv-wasm/src/lib.rs
  - packages/criv-likec4/**
  - .obsidian/plugins/criv/**
  - extensions/vscode-criv/**
  - package.json
  - package-lock.json
  - mise.toml
---

# LikeC4 As The Architecture Source And Renderer

## Context

[[0026-mermaid-c4-diagrams-as-vault-content|ADR-0026]] and
[[0032-c4-files-as-architecture-artifacts|ADR-0032]] made Mermaid C4 and DOT
the first standalone architecture formats. The Rust parser in
`src/c4.rs` validates a useful subset, but the two editor packages must own
separate Mermaid and Graphviz render paths. The formats also cannot provide one
model with named focused views and progressive relationship disclosure.

[[0030-dot-for-generated-code-architecture|ADR-0030]] moved the complete Code
graph to DOT after the full symbol graph exceeded Mermaid layout limits. That
graph is faithful but too large for normal architecture review. The useful Code
boundary is the language module and its imports, not every class, function,
method, and call.

LikeC4 supplies one text DSL, model validation, named views, a public computed
model API, and a React renderer. It is a Node.js tool and has no maintained Rust
parser. Keeping an independent Rust parser would duplicate a changing grammar.
Making an editor process authoritative would make CI results depend on an open
editor. Neither boundary is acceptable.

## Decision

Use one LikeC4 workspace in `docs/architecture/` for each criv vault. LikeC4
DSL in `.c4` files is the only authored architecture source. Agents author the
DSL. LikeC4 is the only visual renderer. Do not parse or render Mermaid C4 or
DOT, and do not extract Mermaid C4 blocks from Markdown.

This is a hard cutover. The implementation agent converts existing Mermaid and
DOT artifacts in the same change. Do not add a migration command, a legacy
reader, a compatibility renderer, or silent conversion behavior.

Require Node.js 26.5.1 and exact `likec4` 1.59.2, `react` 19.2.8, and
`react-dom` 19.2.8 packages when a vault contains LikeC4 source. Resolve them
from the repository lockfile and local `node_modules`. Normal checks and editor
views must not download packages, use a global command, run an update check, or
fetch a remote icon. Vaults without `.c4` source do not require Node.js.

The Rust CLI starts a criv-owned Node bridge. The bridge uses only public
LikeC4 package exports and returns a versioned criv JSON protocol. It validates
the complete workspace, normalizes diagnostic paths and ranges, and returns the
layouted elements, relationships, named views, and links that criv needs. Rust
owns process limits, version checks, deterministic ordering, policy checks,
source-target checks, interface drift, State publication, and user diagnostics.
LikeC4 owns DSL syntax and model semantics.

Treat one local LikeC4 `link` with the label `source` as the criv source anchor.
The target uses the existing criv source-target syntax. Other LikeC4 links
remain ordinary architecture links. criv validates source links and records
their graph and interface-hash edges. Editor adapters open validated source
links through host APIs. They do not place command URIs in the model.

Add language-native module nodes to the source graph:

- Rust uses crates and `mod` declarations, including inline and file-backed
  modules.
- TypeScript and JavaScript use ES modules and declared namespaces.
- Python uses importable modules and packages.
- Go uses packages from `package` declarations across their files.

Files remain source locations. Module identities and import relationships form
the generated Code architecture. Do not generate architecture nodes for
classes, functions, methods, or calls. Generated LikeC4 source contains the
complete module model and one or more focused named views. It does not create
one exhaustive rendered view.

Create one shared TypeScript package under `packages/criv-likec4/`. It owns the
bridge protocol types, public LikeC4 model adapter, React renderer, themes,
named-view selection, validated source-link events, live-model replacement, and
SVG export. The Obsidian and VS Code packages contain thin host adapters. Hosts
watch source files, debounce changes, attach a monotonic revision, ignore stale
responses, and replace and dispose the LikeC4 model. Browser bundles contain all
runtime parser, renderer, layout, font, icon, and WebAssembly assets.

The VS Code webview uses a strict content security policy and message protocol.
The Obsidian view uses the same shared renderer in its content container. Both
render offline after installation, follow the host theme, switch named views,
refresh live source, pan and zoom, open source links, and export SVG. Neither
editor starts the CLI or needs a user-installed Node.js runtime.

Serialize the normalized LikeC4 workspace, elements, relationships, views,
source links, modules, and module imports in `.criv/state.json`. Bump the State
schema because the old C4 node identity and format fields are removed. Emit
readability diagnostics as warnings. Syntax, model, missing dependency,
protocol, source-target, and deterministic-state failures are errors.

Pin every direct package exactly and commit npm lockfile version 3. CI installs
with `npm ci`, then runs validation and editor tests without network access.
Record final bundle sizes, startup time, and first-view time. Scan final bundles
for licenses and required notices. Do not bundle Playwright browsers or native
build tools in either editor.

## Consequences

Architecture authors and agents use one model and one renderer. Named views no
longer duplicate elements across independent diagrams. The CLI remains the
entry point for deterministic repository validation, while LikeC4 supplies the
grammar and model implementation that it owns.

Repositories with architecture sources gain a required Node.js toolchain and a
large locked package graph. Missing tools fail clearly and never trigger a
download. Released editor bundles stay self-contained and do not inherit the
CLI runtime requirement.

Existing Mermaid and DOT artifacts become invalid immediately. The change that
implements this decision must migrate this repository and all installed
fixtures, tests, docs, state projections, and editor assets together.

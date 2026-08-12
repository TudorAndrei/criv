---
id: ADR-0100
kind: decision
title: Agent-Authored Language-Independent C4 Architecture
status: accepted
date: 2026-08-12
supersedes:
  - ADR-0031
  - ADR-0044
  - ADR-0077
  - ADR-0099
governs:
  - src/*.rs
  - crates/criv-wasm/src/lib.rs
  - packages/criv-likec4/**
  - packages/criv-editor-state/**
  - .obsidian/plugins/criv/**
  - extensions/vscode-criv/**
  - assets/skills/c4-authoring/SKILL.md
  - .agents/skills/c4-authoring/SKILL.md
  - assets/likec4-bridge.mjs
  - assets/likec4-contract.json
  - criv.toml
  - package.json
  - package-lock.json
  - mise.toml
policy:
  patterns:
    - id: no-automatic-likec4-source-generator
      language: rust
      rule: |
        all:
          - kind: identifier
          - regex: '^(architecture_code|ArchitectureCodeConfig|c4_code|C4Code|write_code_architecture|for_all_indexed_sources_likec4)$'
      message: The coding agent authors LikeC4 source; criv must not generate it from source facts.
    - id: no-likec4-root-import
      language: typescript
      pattern: 'import $$$IMPORTS from "@criv/likec4"'
      message: Import an explicit @criv/likec4 protocol or renderer entry.
    - id: no-likec4-node-import
      language: typescript
      pattern: 'import $$$IMPORTS from "@criv/likec4/node"'
      message: The embedded criv bridge is the only Node.js LikeC4 compiler.
    - id: no-host-architecture-model-builder
      language: typescript
      rule: |
        any:
          - pattern: function architectureModel($$$ARGS) { $$$BODY }
          - pattern: function c4Artifacts($$$ARGS) { $$$BODY }
          - pattern: function registeredPatterns($$$ARGS) { $$$BODY }
      message: Wasm returns architecture, C4 artifacts, and registered patterns as ready editor projections.
    - id: no-host-raw-likec4-read
      language: typescript
      pattern: $ARCHITECTURE.model.raw
      message: Editor hosts must pass the Wasm LikeC4 projection without reading the raw State model.
    - id: no-likec4-version-cast
      language: typescript
      pattern: '$VALUE as "1.59.2"'
      message: Wasm validates the LikeC4 version; a TypeScript cast cannot establish the contract.
    - id: obsidian-c4-close-needs-renderer-disposal
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async onClose'
          - not:
              has:
                pattern: this.previewRevisions.dispose()
                stopBy: end
      message: Closing an Obsidian C4 leaf must dispose its complete preview revision lifecycle.
---

# Agent-Authored Language-Independent C4 Architecture

## Context

[[0099-enforce-shared-likec4-and-wasm-adapter-boundary|ADR-0099]] keeps one
LikeC4 workspace and a strict shared renderer and Wasm boundary. It retains an
older rule that lets criv generate Code architecture from source modules and
imports. [[0077-c4-standard-alignment-for-the-likec4-workspace|ADR-0077]] also
recommends that generated form when a hand-authored Code model costs too much.

The generated form does not follow the intended architecture workflow. It
creates one view for each programming language, uses source identities as
architecture names, and gives the configured `architecture.code.title` no
effect. It produces a source inventory in LikeC4 syntax, not an architecture
model with names, responsibilities, boundaries, relationships, and titles that
explain the software.

The C4 model is language-independent and uses several related levels of zoom.
The complete model explains one software system. Each diagram tells one story
at one level. A Code diagram is an optional zoom into one important component.
It is not a whole-repository inventory grouped by programming language.

Only the coding agent has enough context to choose useful architecture names,
responsibilities, view scopes, and titles. A deterministic source scanner can
provide evidence, but it cannot make those design choices safely.

[[0031-text-first-c4-architecture-formats|ADR-0031]] correctly keeps
architecture source reviewable as text, but it also assigns deterministic
architecture generation to criv. [[0044-vault-write-confinement|ADR-0044]]
correctly confines every repository mutation, but it names generated Code
architecture as one of those mutations. This decision must retain their general
rules while removing that obsolete write path.

## Decision

Use one agent-authored LikeC4 workspace under the configured vault documents
directory. The coding agent owns every architecture element name,
responsibility, relationship, source link, view scope, and view title. The
complete workspace describes the software architecture without a programming
language boundary.

Keep System Context, Container, Component, Code, Dynamic, and Deployment views
at one level each. A view title names its level and the system, container,
component, or workflow in scope. Use several focused views when the full model
has several stories. Do not make one view for each programming language.

A Code view is optional and has one component as its scope. The agent selects
only the code elements that help explain that component. These elements can be
modules, classes, interfaces, functions, or data structures. Source graph
modules, symbols, calls, and imports are evidence for the agent. They do not
become C4 elements automatically.

Retain the level-separation, external-element style, deployment ownership,
component roll-up, relationship-label, dynamic-view, and deployment-view rules
from ADR-0077. [[0080-co-locate-primary-likec4-views-with-their-models|ADR-0080]]
remains the authority for the location of primary views.

Remove the complete automatic LikeC4 source path. Remove
`[architecture.code]`, the refresh writer, the source-to-LikeC4 serializer, and
`criv query c4-code`. This is a hard cutover. A repository that still has
`[architecture.code]` fails configuration loading with an instruction to
delete the table and let the coding agent author the workspace. Do not add a
default title, a compatibility reader, a source scaffold, or a migration
command.

criv loads the agent-authored workspace, starts the embedded LikeC4 bridge,
validates model rules and source links, reports drift, and publishes the
validated model in State. LikeC4 owns the DSL, layout, and visual output. The
coding agent owns architecture meaning. Update the shipped C4 authoring skill
to enforce this workflow.

Retain the text-first source rule from ADR-0031. LikeC4 source is plain text
that an agent can write, a human can review in a diff, and criv can validate
without an editor. Rendered images, browser state, editor state, and manual
canvas positions are projections. They are not architecture sources.

Retain the complete repository write-confinement rule from ADR-0044. Every criv
repository mutation uses the helpers in `src/util.rs` with the repository root,
an allowed directory, and a repository-relative destination. Writes reject
absolute paths, parent traversal, and symlink components, and publish through
atomic replacement. The allowed directory defines command scope, not safety.
`check --fix` can rewrite every Markdown file that its rumdl configuration
selects inside the repository. State, snapshots, and caches keep their narrow
`.criv` scope. `criv init` also uses the confined helpers. There is no generated
architecture write scope.

Retain the runtime and package boundary from ADR-0099. The embedded
`assets/likec4-bridge.mjs` file is the only Node.js compiler. The shared
`@criv/likec4` package exports only its protocol and renderer entries.
`assets/likec4-contract.json` is the only checked-in owner for the protocol,
Node.js, and LikeC4 versions.

Keep State validation and editor projection in `criv-wasm`. A loaded revision
returns prepared editor projections and never the raw State document. A present
architecture value must pass protocol, version, workspace, model, view, and
source-target validation. Invalid architecture rejects the complete revision.
Keep the stable State, architecture, runtime, and disposal error codes.

Keep `@criv/editor-state` limited to the injected Wasm loader, stable errors,
generation order, replacement, and exact disposal. Keep synchronous model
replacement, view selection, navigation, source events, SVG export, React
unmount, and idempotent disposal in `@criv/likec4/renderer`. Keep files, editor
events, status, panels, leaves, commands, and host cleanup in each editor host.

Keep `npm run check:editor-contracts` as the direct package and Rust-Wasm gate.
Both editor builds and tests run this gate. Keep the direct protocol, model,
renderer, navigation, loader, revision, recovery, and disposal tests from
ADR-0099. Add configuration and CLI tests that prove the hard removal, and a
refresh test that proves criv does not create architecture source.

Enforce the generator removal and the shared package bypass forms with the
inline policies in this ADR. Direct package tests continue to reject forbidden
dependencies that a structural pattern cannot safely scope to one package.

The architecture loading implementation is owned by `src/likec4.rs`, the
configuration cutover is owned by `src/config.rs`, and the command removal is
owned by `src/query.rs`.

## Consequences

Architecture source contains deliberate names and views that explain the
system. It no longer changes shape because the repository uses one or several
programming languages.

The coding agent must maintain the model. criv cannot create a first diagram
from source files. The source graph still gives the agent current code facts,
and source links keep selected architecture elements connected to the
implementation.

Repositories that use `[architecture.code]` must remove that table and author
their LikeC4 workspace. Calls to `criv query c4-code` fail as an unknown query.
There is no silent compatibility behavior.

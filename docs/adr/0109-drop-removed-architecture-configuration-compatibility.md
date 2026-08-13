---
id: ADR-0109
kind: decision
title: Drop removed architecture configuration compatibility
status: accepted
date: 2026-08-13
supersedes:
  - ADR-0100
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

# Drop Removed Architecture Configuration Compatibility

## Context

[[0100-agent-authored-language-independent-c4-architecture|ADR-0100]] removed
automatic architecture generation. It kept one compatibility parser only to
reject `[architecture.code]` with migration guidance. The hard cutover is now
complete. This parser, its module, and its negative tests no longer have a
current product function.

## Decision

Remove the architecture compatibility module and its configuration field.
Unknown removed architecture tables have the same behavior as other unknown
removed configuration fields: criv does not load them into its product model.
Do not keep tests that only prove the old migration error.

Keep all other architecture ownership and editor boundaries from ADR-0100.
The policy definitions above are their current owners.

## Consequences

The configuration model contains only supported product settings. A repository
that still contains `[architecture.code]` gets no special migration error, and
criv does not use the table or create architecture source from it.

---
id: ADR-0155
kind: decision
title: Remove LikeC4 From criv
status: accepted
date: 2026-10-31
supersedes:
  - ADR-0034
  - ADR-0035
  - ADR-0045
  - ADR-0080
  - ADR-0089
  - ADR-0091
  - ADR-0096
  - ADR-0109
  - ADR-0128
  - ADR-0147
governs:
  - src/adr.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
  - src/check.rs
  - src/discovery/**/*.rs
  - src/enforce.rs
  - src/init.rs
  - src/init/templates.rs
  - src/install.rs
  - src/install/**/*.rs
  - src/lib.rs
  - src/policy_scan.rs
  - src/query.rs
  - src/repository.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/state.rs
  - src/state/**/*.rs
  - src/structural.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-state-wire/src/**/*.rs
  - crates/criv-wasm/src/**/*.rs
  - packages/criv-editor-state/src/**/*.ts
  - extensions/vscode-criv/src/**/*.ts
  - .obsidian/plugins/criv/src/**/*.ts
  - assets/skills/**/SKILL.md
  - package.json
policy:
  patterns:
    - id: no-caller-owned-reconciliation-snapshot
      language: rust
      rule: |
        any:
          - pattern: struct TransactionSnapshot { $$$FIELDS }
          - pattern: struct PathSnapshot { $$$FIELDS }
      message: Reconciliation callers must use the shared Snapshot owner in src/adr/reconcile_transaction.rs.
    - id: no-private-adr-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::adr::(source_reconcile|reconcile_transaction)(::|$)'
      message: Callers must use the src/adr.rs interface, not a private reconciliation child.
    - id: no-private-install-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::install::(editor|skills)(::|$)'
      message: Callers must use the src/install.rs interface, not a private installation child.
    - id: no-private-repository-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::repository::filesystem(::|$)'
      message: Callers must use the src/repository.rs interface, not its private filesystem child.
    - id: confined-repository-mutations-only
      language: rust
      rule: |
        all:
          - any:
              - pattern: std::fs::write($$$ARGS)
              - pattern: fs::write($$$ARGS)
              - pattern: std::fs::rename($$$ARGS)
              - pattern: fs::rename($$$ARGS)
              - pattern: std::fs::remove_file($$$ARGS)
              - pattern: fs::remove_file($$$ARGS)
              - pattern: std::fs::remove_dir_all($$$ARGS)
              - pattern: fs::remove_dir_all($$$ARGS)
              - pattern: std::fs::create_dir_all($$$ARGS)
              - pattern: fs::create_dir_all($$$ARGS)
              - pattern: File::create($$$ARGS)
              - pattern: OpenOptions::new()
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: Repository mutations must use the RepositoryFiles interface in src/repository.rs; direct filesystem mutation is test-only.
    - id: no-native-linter-subprocess
      language: rust
      rule: |
        any:
          - pattern: Command::new("ruff")
          - pattern: Command::new("oxlint")
          - pattern: Command::new("eslint")
      message: ADR-0046 keeps native language linters outside criv enforce and the runtime.
    - id: no-architecture-runtime-subprocess
      language: rust
      rule: |
        any:
          - pattern: Command::new("node")
          - pattern: Command::new("npx")
          - pattern: Command::new("likec4")
      message: criv has no architecture compiler runtime; ADR validation and enforcement are independent.
    - id: no-removed-command-modules
      language: rust
      rule: |
        any:
          - pattern: mod search;
          - pattern: mod measurement;
      message: ADR-0072 and ADR-0082 removed core measurement and the standalone search command.
    - id: no-obsolete-init-flags
      language: rust
      rule: |
        any:
          - pattern: '"no-obsidian"'
          - pattern: '"no-vscode"'
          - pattern: '"no-hooks"'
          - pattern: '"force-hooks"'
      message: Hook and editor installation remain separate from criv init.
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
    - id: no-state-pattern-fallback
      language: typescript
      rule: |
        any:
          - pattern: Object.keys($OBJ.patterns ?? {})
          - pattern: Object.keys($OBJ?.patterns ?? {})
      message: Registered patterns come from validated State, not from match keys.
    - id: no-editor-state-schema-literal
      language: typescript
      pattern: '"criv.state.v1"'
      message: State schema validation stays in the Rust-Wasm boundary.
    - id: no-editor-source-lookup-index
      language: typescript
      rule: |
        kind: identifier
        regex: '^(buildSourceTargetIndex|resolveSourceEntry|canonicalByLegacy)$'
      message: The canonical Wasm revision owns source-target lookup and legacy alias resolution.
    - id: no-host-pattern-projection-builder
      language: typescript
      pattern: function registeredPatterns($$$ARGS) { $$$BODY }
      message: Wasm supplies registered patterns as a prepared editor projection.
---

# Remove LikeC4 From criv

## Context

criv combines ADR governance with a LikeC4 compiler, architecture graph rows,
layout State, a shared React renderer, editor previews, and architecture
runtime installation requirements. The proposed Gherkin integration would add
scenario parsing and derived dynamic views to that same tool.

The owner chose to remove LikeC4 from this codebase. criv remains the ADR
system of record and enforcement engine. Architecture compilation, rendering,
and behaviour-to-diagram derivation are outside its scope. This is a removal,
not a replacement of the existing compiler with another architecture
implementation inside criv.

Several decisions mix architecture-specific rules with still-required query,
source identity, diagnostic, installation, and repository safety rules. Retiring
those decisions must not silently retire the supporting ADR capabilities.

## Decision

Keep ADR identity, publication, reconciliation, lifecycle, supersession,
immutability, governance scopes, and structural policy enforcement in criv.
Keep documentation validation, source indexing, imports, source relationships,
source selectors, queries, diagnostics, and incremental State publication that
support those capabilities. Keep the general Obsidian and VS Code companions
for documentation, source navigation, assets, policies, and diagnostics.

Remove the complete LikeC4 runtime from criv's implementation: artifact discovery and parsing,
bridge execution, architecture validation and drift, architecture State and
graph rows, architecture-specific Wasm projections and errors, renderer
packages, previews, view lifecycle helpers, C4 editor registrations, and their
exclusive tests and fixtures. Remove the LikeC4 and React dependencies, the C4
authoring skill from `criv init`, and the architecture build from the documentation site.
Keep the existing `docs/architecture/` files as repository architecture
documentation. They are authored documents, not part of criv's implementation.
Keeping them does not restore automatic discovery, compilation, validation,
State projection, or previews in criv.
Neither a `.c4` file nor a `.feature` file activates an integration in criv.
Such files may still be ordinary indexed source files when source roots select
them; criv assigns them no architecture or scenario semantics.

criv does not own a Gherkin parser, scenario-to-view derivation, architecture
integration State, or architecture editor experiences. Do not retain an
architecture adapter, compatibility layer, or plugin framework for the removed
functionality. A scenario or diagram failure cannot prevent independent ADR
validation and policy enforcement.

Retain the non-architecture rules of the superseded decisions:

- From ADR-0034, use semantic AST-aware source identity for governance, source
  targets, queries, and State. Keep line ranges as display metadata, not
  canonical symbol identity. Keep existing compatibility resolution and
  guidance; ADR-0130 continues to own the shared identity grammar.
- From ADR-0035, retain the VS Code-compatible companion as a local State
  consumer with source navigation, diagnostics, and trust-gated CLI commands.
  Remove only its C4 rendering responsibility. ADR-0071 continues to own
  canonical Wasm validation, lookup, ranking, and packaging.
- From ADR-0045, attach note positions once at the note parser using the
  frontmatter offset. Diagnostics and heading identities use file lines.
  Invalid-frontmatter bodies retain their zero offset.
- From ADR-0089, retain capability-directed Wikilink dispatch and query loading.
  Snapshot-only queries do not load a vault. Docs-only queries do not build
  Source. Source queries use the shared source catalog lifecycle. The docs-only
  set remains `next-adr-id`, `cited-by`, `orphan-docs`, and documentation- or
  decision-restricted `nodes`.
- From ADR-0091, keep validated registered-pattern projections, Rust-Wasm
  schema validation, and canonical source-target lookup. The policies above
  retain those guards; retire the C4 parser, import, and preview-only guards.
- From ADR-0096, retain the surviving workspace State-status stream and its
  loading, ready, missing, invalid, and unavailable states. ADR-0083 remains
  authoritative for latest-load ownership and exact revision disposal. Retire
  diagram retention, preview bindings, rendering, and view-selection rules.
- From ADR-0109, keep removed architecture configuration outside the product
  model, with the ordinary unknown-field behaviour and no migration parser.
  Keep Wasm as the owner of surviving editor State projections.
- From ADR-0128, retain ADR, installation, Git, and repository-file ownership,
  transaction ordering, rollback, permissions, native-linter exclusion, removed
  commands, and obsolete Init flags. The policies above retain its applicable
  enforcement, replacing local-bridge execution with no architecture runtime.
  The private filesystem implementation remains outside caller mutation scopes.
- Retire the remaining architecture-only ownership, preview, and runtime
  compatibility rules of ADR-0080, ADR-0109, and ADR-0147. They are historical,
  no longer requirements for this codebase.

Keep `criv.state.v1` for the surviving graph and inventory shapes. New output
omits `architecture` and architecture-only graph kinds. Readers ignore unknown
top-level fields as before; old graph rows with retired enum kinds require a
`criv watch --once` rebuild. Independent `criv check` no longer reads prior
State merely to compare architecture interfaces, so stale architecture State
cannot block ADR validation. Snapshot diff continues reading kinds as text.
Update and rebuild the companions together with their Wasm runtime. Do not
retain an architecture compatibility reader or tests of deleted error codes.

Leave accepted historical ADR files byte-for-byte unchanged. Preserve the
previous architecture implementation and research in Git history. Retire
architecture-only implementation source, shipped runtime skills, dependencies,
tests, and fixtures rather than renaming them to keep obsolete functionality
alive. Preserve the current repository architecture files and repository-only
authoring guidance.

## Consequences

criv's validation path has no Node.js, LikeC4, layout, or renderer dependency.
Its policy and documentation/source capabilities remain independently usable.
Editor source navigation still uses Wasm and its packaged-runtime smoke tests;
LikeC4-only tests and unused preview helpers disappear with their owners.

This is a breaking removal for architecture consumers. Existing diagrams in
consuming repositories are not deleted automatically, and the first State
rebuild removes their architecture rows. criv no longer supplies their
compilation, validation, or preview workflow. Its smaller scope keeps
architecture failures and runtime complexity outside the ADR authority.

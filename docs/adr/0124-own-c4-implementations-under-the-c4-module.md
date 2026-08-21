---
id: ADR-0124
kind: decision
title: Own C4 Implementations Under The C4 Module
status: accepted
date: 2026-08-20
supersedes:
  - ADR-0122
  - ADR-0123
governs:
  - src/adr.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
  - src/c4.rs
  - src/c4/**/*.rs
  - src/check.rs
  - src/discovery/**/*.rs
  - src/enforce.rs
  - src/init.rs
  - src/init/templates.rs
  - src/install_editor.rs
  - src/lib.rs
  - src/policy_scan.rs
  - src/query.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/state.rs
  - src/state/**/*.rs
  - src/structural.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-wasm/src/**/*.rs
  - assets/likec4-bridge.mjs
  - extensions/vscode-criv/src/diagnostics/model.ts
  - extensions/vscode-criv/src/diagnostics/publisher.ts
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
    - id: no-private-c4-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::c4::(artifact|likec4)(::|$)'
      message: Callers must use the src/c4.rs interface, not a private C4 child.
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
      message: Repository mutations must use the confined helpers in src/util.rs; direct filesystem mutation is test-only.
    - id: no-native-linter-subprocess
      language: rust
      rule: |
        any:
          - pattern: Command::new("ruff")
          - pattern: Command::new("oxlint")
          - pattern: Command::new("eslint")
      message: ADR-0046 keeps native language linters outside criv enforce and the runtime.
    - id: no-global-likec4-subprocess
      language: rust
      rule: |
        any:
          - pattern: Command::new("npx")
          - pattern: Command::new("likec4")
      message: ADR-0074 requires the locked local Node bridge, not npx or a global LikeC4 command.
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
      message: ADR-0054 and ADR-0087 removed hook and editor actions from criv init.
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
---

# Own C4 Implementations Under The C4 Module

## Context

The C4 artifact parser and the LikeC4 bridge implement one C4 loading
operation, but they appear as separate root modules. `src/vault.rs` imports
both modules and assembles their results. This makes the Vault caller know the
internal split between file classification and workspace compilation.

[[0105-owner-scoped-rust-module-layout|ADR-0105]] requires the file tree and
Rust module tree to show the same owner. The artifact parser and the bridge
must stay separate because they own different complexity. The parser owns
file-local classification. The bridge owns a bounded Node.js process and a
versioned protocol. They are sibling implementations of one C4 interface.

[[0122-byte-spans-and-lsp-diagnostic-ranges|ADR-0122]] and
[[0123-own-reconciliation-under-the-adr-module|ADR-0123]] name the old
`src/likec4.rs` path in their effective scopes. Accepted ADRs are immutable.
This decision must update that scope and retain their diagnostic,
reconciliation, and runtime rules.

## Decision

Make `src/c4.rs` the only crate-level C4 interface. Move artifact
classification to `src/c4/artifact.rs`. Move the LikeC4 process and protocol
implementation to `src/c4/likec4.rs`. Declare both child modules as private.
They are siblings. The bridge is not a child of the artifact parser.

The C4 interface exposes the artifact data, workspace data, diagnostics, file
parser, and workspace loader that callers need. Vault loading imports only the
C4 interface. Check and State also use only this interface. Remove the root
LikeC4 module declaration. Do not keep a forwarding module at the old path.

Keep C4 classification, the exact Node.js and LikeC4 contract, process timeout,
output limit, diagnostic normalization, deterministic ordering, State output,
CLI diagnostics, and public behavior unchanged. The move changes ownership,
not runtime behavior.

Record `criv::c4` as the parent interface in the Code architecture map. Record
`criv::c4::artifact` and `criv::c4::likec4` as sibling implementations under
the C4 service. Show calls from Vault, Check, and State to the parent interface
only. Show the parent interface calling each child. Keep each source link on
the file that implements the stated responsibility.

### Retained diagnostic location contract

Retain the complete diagnostic location rules from ADR-0122. The core model
uses an optional zero-based, end-exclusive UTF-8 byte span over the complete
file. A producer omits the span when it cannot prove valid UTF-8 boundaries.
The existing optional `line` remains one-based and identifies the span start.

Keep JSON output as a top-level array with all current fields. Its optional
`range` uses zero-based UTF-16 Language Server Protocol positions. The VS Code
adapter prefers this range and keeps the full-line fallback. The GitHub adapter
derives its one-based annotation positions from the same span. Conversion has
one tested implementation for Unicode, supplementary characters, CRLF,
multiple lines, and empty ranges.

Preserve exact locations from rumdl, structural matches, LikeC4, and vault
parsing when source information proves them. Do not put spans, offsets,
excerpts, or source contents in State. Do not add `miette` as part of this
migration.

### Retained reconciliation and runtime contract

Retain the complete reconciliation ownership from ADR-0123. `src/adr.rs` is
the interface for ADR identity reconciliation and governed source
reconciliation. Its private children keep their separate planning, receipt,
proof, commit, and error contracts. The shared transaction child restores the
complete Git index, file contents, permissions, and absent paths. Enforcement
uses only the ADR interface.

Retain all runtime boundary policies from ADR-0123 on the current module tree.
Repository mutations use confined helpers. Native linters stay outside the
runtime. LikeC4 compilation uses the locked local Node bridge. Removed command
modules and obsolete Init flags stay absent. Direct `git2` use stays behind
`src/git.rs`. Add one policy that prevents callers from importing either
private C4 child.

No command, receipt schema, transaction order, rollback behavior, diagnostic
wire shape, State schema, or user output changes.

## Consequences

Callers learn one C4 interface. The parent hides the choice and order of file
classification and workspace compilation. Each child keeps its focused tests
and complex implementation.

The source tree and Code architecture map now show the same ownership. Future
C4 changes have one crate-level seam, while artifact and process changes stay
local to their separate sibling modules.

ADR-0122 and ADR-0123 become historical only because their source scopes name
the moved file. Their behavior remains effective through this decision.

## Alternatives Considered

### Keep separate root modules

Rejected. Vault would continue to assemble one C4 operation from two root
interfaces.

### Put the bridge under the artifact parser

Rejected. Process execution and protocol validation are not artifact parsing.
The two implementations need one parent, not a parent-child relation to each
other.

### Keep a forwarding LikeC4 module

Rejected. A forwarding module adds a shallow interface only to preserve an
internal path.

---
id: ADR-0125
kind: decision
title: Own Installation Implementations Under The Install Module
status: accepted
date: 2026-08-20
supersedes:
  - ADR-0087
  - ADR-0124
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
  - src/install.rs
  - src/install/**/*.rs
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
  - .github/workflows/release.yml
  - README.md
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
    - id: no-private-install-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::install::(editor|skills)(::|$)'
      message: Callers must use the src/install.rs interface, not a private installation child.
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
      message: ADR-0054 and ADR-0125 keep hook and editor actions out of criv init.
    - id: git2-only-through-git-boundary
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^git2::'
      message: ADR-0058 puts git2 behind src/git.rs and requires criv-owned values at caller boundaries.
---

# Own Installation Implementations Under The Install Module

## Context

Generated skill installation and inventory are in `src/generated_skills.rs`.
Editor installation is in `src/install_editor.rs`. Both are installation
implementations, but they appear as unrelated root modules. `src/init.rs`,
`src/check.rs`, and `src/lib.rs` must know which implementation owns each
operation.

The implementations must stay separate. Skill installation reads and writes
confined repository content and maintains the generated skill lifecycle.
Editor installation finds one selected external editor and starts its local
extension installation command. Combining their implementations would mix two
different dependencies and two different user operations.

[[0105-owner-scoped-rust-module-layout|ADR-0105]] requires the file tree and
Rust module tree to show one owner. [[0087-keep-editor-setup-out-of-init|ADR-0087]]
keeps editor setup separate from vault initialization.
[[0124-own-c4-implementations-under-the-c4-module|ADR-0124]] names the old
editor installation path in its effective runtime scope. Accepted ADRs are
immutable. This decision must update the ownership paths and retain all
effective behavior from both decisions.

## Decision

Make `src/install.rs` the only crate-level installation interface. Move
generated skill installation and inventory to `src/install/skills.rs`. Move
editor discovery and installation to `src/install/editor.rs`. Declare both
child modules as private siblings. Do not keep forwarding modules at the old
paths.

The installation interface exposes the skill install operation, best-effort
skill inventory, skill publication facts, editor options, and editor install
operation that callers need. `src/init.rs`, `src/check.rs`, and `src/lib.rs`
use only this interface. They must not import either child implementation.

Keep the command flows separate. `criv init` remains the vault initialization
command and can install or refresh generated skills. `criv install-editor`
remains the only editor installation command. Do not combine these commands or
make either command call the other.

Keep all generated skill template identity, marker handling, create-only and
refresh behavior, inventory status, `.agents/skills` publication, and
`.claude/skills` link-or-copy behavior unchanged. Keep all Init output and the
best-effort stale-skill advisory unchanged.

Keep editor CLI discovery, bundled VSIX validation, dry-run behavior, process
execution, standard output and standard error reporting, exit-status handling,
and CLI output unchanged.

Record one Installation component in the Component architecture map. Record
`criv::install` as its parent Code interface. Record `criv::install::skills`
and `criv::install::editor` as private sibling implementations. Keep
`criv::init` as a separate command workflow in the same Installation
component. Show command routing and validation calls to the parent interface,
then show the parent interface calling each child.

### Retained editor installation contract

Retain the complete editor separation rules from ADR-0087. `criv init` is
limited to the criv vault, local generated State, and generated agent skills.
It must not create, update, or remove `.obsidian` or `.vscode` files. It has no
editor-specific options.

Keep the optional editor viewer local-only. Do not publish it to the VS Code
Marketplace or Open VSX. Build one `vscode-criv.vsix` for each criv release and
put it next to the `criv` executable in each release archive.

Keep only these explicit installation commands:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Keep `--editor` required and accept only `code` or `cursor`. Keep `--dry-run`.
Resolve only the fixed sibling `vscode-criv.vsix` file and run only the
selected editor local extension installation command. Do not accept a package
path, find an editor automatically, download a package, or install into more
than one editor.

The Obsidian companion remains a maintained source package in this repository.
Init does not copy it into another repository. All editor companions consume
`.criv/state.json`; the CLI remains authoritative for graph generation,
validation, and enforcement.

### Retained C4 ownership contract

Retain the complete C4 ownership rules from ADR-0124. `src/c4.rs` is the only
crate-level C4 interface. `src/c4/artifact.rs` and `src/c4/likec4.rs` are
private sibling implementations. Vault, Check, and State use only the C4
interface. No forwarding module exists at an old path.

Keep C4 classification, the exact Node.js and LikeC4 contract, process timeout,
output limit, diagnostic normalization, deterministic ordering, State output,
CLI diagnostics, and public behavior unchanged.

### Retained diagnostic location contract

Retain the complete diagnostic location rules from ADR-0124. The core model
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

Retain the complete reconciliation ownership from ADR-0124. `src/adr.rs` is
the interface for ADR identity reconciliation and governed source
reconciliation. Its private children keep their separate planning, receipt,
proof, commit, and error contracts. The shared transaction child restores the
complete Git index, file contents, permissions, and absent paths. Enforcement
uses only the ADR interface.

Retain all runtime boundary policies from ADR-0124 on the current module tree.
Repository mutations use confined helpers. Native linters stay outside the
runtime. LikeC4 compilation uses the locked local Node bridge. Removed command
modules and obsolete Init flags stay absent. Direct `git2` use stays behind
`src/git.rs`. Add one policy that prevents callers from importing either
private installation child.

No command, option, receipt schema, transaction order, rollback behavior,
diagnostic wire shape, State schema, or user output changes.

## Consequences

Callers learn one installation interface. Skill lifecycle changes stay in the
skills child. Editor process changes stay in the editor child. The parent
interface keeps those details out of Init, Check, and command routing.

The source tree and Code architecture map now show the same ownership. The
separate `init` and `install-editor` user flows remain clear.

ADR-0087 and ADR-0124 become historical only because their source scopes name
the moved editor file. Their behavior remains effective through this decision.

## Alternatives Considered

### Keep separate root modules

Rejected. The source tree would continue to hide their shared installation
owner.

### Put editor installation under Init

Rejected. Editor setup is an explicit editor-level choice and must stay outside
the repository initialization flow.

### Combine skill and editor implementations

Rejected. Their dependencies, errors, and lifecycle rules are different. They
need one parent interface, not one implementation.

### Keep forwarding modules at the old paths

Rejected. Forwarding modules add shallow interfaces only to preserve internal
path text.

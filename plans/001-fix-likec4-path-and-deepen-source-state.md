# Plan 001: Fix the LikeC4 path and deepen Source and State ownership

> **Executor instructions**: Follow this plan in order. Run every check listed
> below. If a STOP condition occurs, stop and report it. Do not improvise. When
> the work ends, update this plan's row in `plans/README.md`.
>
> **Drift check**: Run `git diff --stat bc1e7e6..HEAD -- assets/likec4-bridge.mjs src/c4/likec4.rs src/lib.rs src/source.rs src/source/graph.rs src/source/graph src/state.rs src/state docs/architecture`. If a file in scope changed, compare it with the excerpts in this plan. Stop if its meaning changed.

## Status

- **Execution**: COMPLETE
- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: None
- **Category**: bug, tests, tech-debt
- **Planned at**: commit `bc1e7e6`, 2026-08-25

## Why this matters

The LikeC4 bridge treats an operating-system path as a URL. A valid vault path
with `#` or `%` can fail before LikeC4 validates the workspace. The public
`--usage-json` command has no direct contract test.

`src/source/graph.rs` mixes generic syntax extraction with Source graph cache
and query work. `src/state.rs` mixes State partition reuse with graph projection.
Move each implementation behind its existing owner interface. Do not change CLI
behavior, serialized State, graph cache format, or editor output.

## Current state

- `assets/likec4-bridge.mjs` runs in the repository root. It resolves the local
  LikeC4 package before it validates a workspace.

  ```js
  // assets/likec4-bridge.mjs:1-8
  import { createRequire } from 'node:module';
  import { dirname, join, relative, resolve } from 'node:path';
  import { pathToFileURL } from 'node:url';
  const require = createRequire(new URL(`file://${process.cwd()}/package.json`));
  ```

- `src/c4/likec4.rs:123-136` starts that bridge with `root` as the current
  directory. This makes the bridge path construction a supported vault-path
  case.

- `src/lib.rs:199-295` maps `usage::spec::CommandMeta` into `JsonCommand`, then
  writes pretty JSON. `src/lib.rs:297-363` tests only `write_usage_spec`.

- `src/source.rs:36-108` owns `SourceState`. Its callers use `refresh_from`,
  `reuse_for_docs`, and read-only query methods. Its child `graph` module stays
  private.

- `src/source/graph.rs:679-956` builds the graph and indexes. The same file at
  `src/source/graph.rs:966-1109` reads and reuses files. Its generic Tree-sitter
  extraction and fallback parsing continue through `src/source/graph.rs:1124-`
  `1644`. `src/source/graph/elixir.rs` owns Elixir meaning already.

- `src/state.rs:189-300` builds, serializes, and publishes State. The same file
  defines invalidation and partition construction at `src/state.rs:312-615`,
  Source, note, and C4 graph projection at `src/state.rs:790-1203`, and policy
  partition construction at `src/state.rs:1218-1314`.

- `docs/adr/0105-owner-scoped-rust-module-layout.md:35-54` requires
  file-plus-directory owner layout. `docs/adr/0126-own-one-source-state-per-refresh.md`
  requires `SourceState` to own one completed Source observation. `docs/adr/0129-own-elixir-graph-meaning-in-the-elixir-module.md:42-94`
  keeps generic graph work in the general graph module and forbids a language
  trait with one adapter. `docs/adr/0130-share-structured-source-identity-text.md:45-52`
  keeps the State wire document unchanged.

## Commands you will need

| Purpose | Command | Expected result |
| --- | --- | --- |
| Format and fix changed files | `mise run fix` | Exit 0 |
| Rust tests | `cargo test --workspace` | Exit 0 |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| Vault check | `cargo run --quiet -- check` | Exit 0 |
| Obsidian tests | `npm --prefix .obsidian/plugins/criv test` | Exit 0 |
| VS Code tests | `npm --prefix extensions/vscode-criv test` | Exit 0 |

## Scope

### In scope

- `assets/likec4-bridge.mjs`
- `src/c4/likec4.rs` and its existing tests
- `src/lib.rs`
- `src/source.rs`, `src/source/graph.rs`, and new private files under
  `src/source/graph/`
- `src/state.rs` and new private files under `src/state/`
- affected Rust tests in the same owner scopes
- `docs/architecture/code/cli.c4`, `docs/architecture/code/state-wire.c4`, and
  other existing Code views that name moved Source or State implementation work

### Out of scope

- State wire schema, source graph cache schema, CLI output, and editor protocol values
- `crates/criv-state-wire`, `crates/criv-wasm`, editor hosts, and package versions
- Elixir language meaning and selector text
- a filesystem trait, a language trait, or a registry of adapters
- accepted ADR edits. If the work needs a behavior decision, stop and propose a
  new ADR instead.

## Git workflow

- Use one branch: `advisor/001-likec4-source-state`.
- Make scoped conventional commits. Suggested commits are `fix(c4): resolve LikeC4 from a file URL`, `test(cli): cover usage JSON export`, `refactor(source): isolate generic extraction`, and `refactor(state): isolate partition construction`.
- Do not push or open a pull request unless the operator asks.

## Steps

### Step 1: Fix LikeC4 local package resolution and add a path contract test

In `assets/likec4-bridge.mjs`, replace the interpolated `file://${process.cwd()}`
construction with `pathToFileURL(join(process.cwd(), 'package.json'))`. Pass that
file URL to `createRequire`. Keep the bridge protocol and local package-only
resolution unchanged.

Add a Rust-level contract test beside the existing LikeC4 tests in
`src/c4/likec4.rs`. Create a temporary vault path that contains URL syntax
characters. Make its minimal local package setup and LikeC4 workspace match the
current bridge test fixture. Assert that the bridge resolves the local package
and returns the normal result. The test must run on all supported hosts.

**Verify**: `cargo test --lib c4::likec4` exits 0.

### Step 2: Test the public `--usage-json` interface

In `src/lib.rs` tests, import `write_usage_json`. Write it to `Vec<u8>`, parse
the result with `serde_json::Value`, and assert representative semantic facts:

- root command name and path are `criv`;
- the Query command and a known Query child have their expected paths;
- a visible flag exposes its choices, default, conflicts, or requirements when
  the usage specification declares them;
- hidden command data does not appear.

Add one command-dispatch test if an existing test pattern can call `run` without
process exit. It must assert that `criv --usage-json` returns valid JSON. Do not
assert the complete pretty-printed string.

**Verify**: `cargo test --lib usage` exits 0. If this filter does not select the
test module, use the narrow test name and record the working command in the
commit message or pull request.

### Step 3: Move generic Source extraction behind a private module

Create a private child module under `src/source/graph/` for generic Tree-sitter
extraction. Give the file a concrete concern name, such as `extract.rs`. Move
generic parser selection, tree walking, signature extraction, and fallback
parsing from `src/source/graph.rs` into it. Keep the child private through the
existing `mod` declaration in `src/source/graph.rs`.

Keep `SourceGraph` in `src/source/graph.rs`. It must still own selected-file
assembly, deterministic order, cache loading and publication, shared storage and
indexes, language selection, generic lookup and traversal, and State-facing
queries. Keep all Elixir-specific rules in `src/source/graph/elixir.rs`.

Expose only the smallest private interface needed for the graph implementation.
It should accept the data needed to parse one selected file and return the
existing `SourceFile` result. Do not add a trait or an adapter. There is one
generic implementation.

Move focused generic parsing tests with the extraction code. Keep full graph,
cache, lookup, caller, and callee tests with `SourceGraph`. Preserve the exact
order and values in `SourceFile`, selector lookup, and graph cache.

**Verify**: `cargo test --lib source` exits 0.

### Step 4: Move State partition construction behind a private module

Create `src/state/` with a private child module named for partition work. Move
`InvalidationFacts`, reverse-dependency construction, partition reuse decisions,
Source, note, C4, and policy partition construction, flattening, and graph
projection helpers from `src/state.rs` into that child.

Keep the `State` interface in `src/state.rs`. It owns construction entry points,
State wire serialization, and snapshot publication. Keep the external function
names and behavior used by refresh and one-shot execution.

The private partition module must return the existing partition result needed by
`State::from_partitions`. It must not expose State wire implementation details to
callers. Preserve deterministic graph row order, stable hashes, reuse behavior,
policy matches, and LikeC4 projection.

Move partition-specific tests beside the new module. Retain end-to-end State
tests at the State interface. Add characterization assertions before moving code
if current tests do not prove the exact serialized State and incremental reuse
results for a fixture.

**Verify**: `cargo test --lib state` exits 0.

### Step 5: Update Code architecture and run full checks

Update only Code architecture views whose source links or descriptions name the
moved implementation work. Show `criv::source::graph` and `criv::state` as the
owner interfaces. Show their new children as private implementation work. Do not
change the authored system, container, or deployment model.

Run formatting once after all Rust and C4 edits. Then run the full Rust and vault
checks. Run the editor tests because State and LikeC4 contracts feed the editor
consumers.

**Verify**: `mise run fix`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo run --quiet -- check`, `npm --prefix .obsidian/plugins/criv test`, and `npm --prefix extensions/vscode-criv test` all exit 0.

## Test plan

- LikeC4: a temporary root with `#` and `%` resolves the local package and
  validates a minimal workspace.
- CLI metadata: parsed usage JSON contains expected visible command and flag
  data. A dispatch test covers `--usage-json` if the current test interface can
  call it.
- Source: existing generic-language parser cases keep exact `SourceFile` data;
  cache reuse, lookup, callers, and callees still pass; Elixir tests prove no
  semantic rule moved out of `elixir.rs`.
- State: cold and incremental State tests keep exact JSON and hash results;
  source, note, C4, and policy partition reuse tests keep their rebuild counts.
- Editors: Obsidian and VS Code tests consume unchanged State and LikeC4 data.

## Done criteria

- [x] `cargo test --workspace` exits 0.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- [x] `cargo run --quiet -- check` exits 0.
- [x] Obsidian and VS Code test commands exit 0.
- [x] A vault root with URL syntax characters passes the LikeC4 bridge test.
- [x] `--usage-json` has a parsed JSON contract test.
- [x] Generic parser signatures and fallback extraction move into `src/source/graph/extract.rs`.
- [x] State source, note, C4, and policy graph projection move into private State modules.
- [x] State wire JSON, graph cache behavior, selector text, and editor values stay unchanged.
- [x] No file outside the declared scope changes, except ignored generated artifacts.
- [x] `plans/README.md` marks Plan 001 DONE.

## STOP conditions

- The LikeC4 bridge test requires a network download or a global LikeC4 package.
- The Source move requires a public child interface, a language trait, or a new adapter.
- The State move changes State JSON, its hash, graph cache schema, or editor protocol data.
- A required Code architecture change needs a new behavior decision instead of a source-link update.
- An accepted ADR needs an edit. Write a new ADR proposal instead.
- A targeted test fails twice after a small, local correction.

## Maintenance notes

Keep generic Source parsing in the private extraction module. Keep language
meaning in its language module. Add another adapter only when a second real
implementation exists.

State partition behavior is compatibility-sensitive. Review deterministic order,
hashes, policy reuse, and editor inputs before accepting later changes. The
LikeC4 bridge must keep resolving only from the repository's local package set.

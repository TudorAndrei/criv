# criv Implementation Plan

## Current State

`criv` has a working Rust workspace with:

- CLI commands for `init`, `check`, `query`, `search`, `watch`, and `enforce`.
- Real TOML/YAML parsing for `criv.toml` and markdown frontmatter.
- Vault scanning, note resolution, wiki-link validation, ADR validation, supersession checks, and pattern ID collision checks.
- A tree-sitter-backed source graph that extracts files, imports, symbols, ranges, containment, and call edges for Rust, TypeScript, JavaScript, Python, and Go, with conservative fallback behavior where parsing fails.
- State writing to `.criv/state.json`.
- Content-addressed local snapshots under `.criv/snapshots/` plus `.criv/latest`.
- Local snapshot diff support through `criv query diff <snapshot-a> <snapshot-b>` plus git-ref diff resolution.
- fff-backed fuzzy file search, grep, partial-path resolution, and source watch integration.
- ast-grep-backed direct search, configured pattern search, ADR policy search, state match storage, check failures, and enforcement.
- Incremental watch rebuilds that reuse unchanged source graph parsing and unchanged pattern match results.
- Obsidian sample-plugin-style TypeScript scaffold with syntax-highlighted source previews, pattern match rendering, drift indicators, ranked source autocomplete, shared link-resolution fixtures, and a Rust WASM helper crate.
- Stage-aware `criv enforce`, backed by validation, native import-policy checks, ast-grep policy checks, and graceful ESLint/Ruff integration.
- mise-managed local validation through `mise run check`, including cargo fmt,
  clippy, workspace tests, actionlint, zizmor, `criv check`, and CI-stage
  enforcement.
- Foundation crates wired for CLI parsing, typed errors, glob matching, markdown event parsing, MIME classification, binary-file detection, stable snapshot hashing, YAML frontmatter parsing, debounced watcher events, tree-sitter parsing, ast-grep search, fff indexing, and optional semantic embeddings.

The Rust backend, Obsidian plugin, release workflow, and local validation
milestones have first implementation passes. Release workflow hardening was
verified by the `v0.3.0` tag-triggered GitHub Actions run, which completed
successfully after building, smoke-testing, packaging, checksumming, attesting,
and publishing the release assets.

## Architecture Direction

Keep the core tool Rust-first and local-only.

- The CLI remains the only programmatic interface.
- Obsidian stays a TypeScript shell following the official sample plugin structure.
- Rust logic needed in Obsidian is compiled to WASM through `crates/criv-wasm`.
- The CLI and plugin should share fixtures for link parsing, state schema, and reference resolution behavior.

Backend boundaries to preserve:

- `source_graph` is the replacement point for tree-sitter-backed extraction.
- `search` is the replacement point for `ast-grep-core` and fff-backed grep/path search.
- `state` owns `.criv/state.json` and snapshot serialization.
- `enforce` owns ADR policy evaluation and future ESLint/Ruff integration.
- `crates/criv-wasm` owns plugin-side Rust helpers.

## Completed Milestones And Hardening Targets

The milestones below have their first implementation pass complete. Keep them as architecture checkpoints and use them to guide hardening work before release.

### 1. Dependency Cleanup and Foundation Crates

Replace hand-rolled infrastructure with proven crates before adding the heavier backends.

Target behavior:

- Use `clap` derive for CLI parsing, help, subcommands, and value enums.
- Use `thiserror` for typed internal errors.
- Consider `miette` for source-span diagnostics after validation spans are richer.
- Replace custom glob matching in `util` with compiled `globset` matchers.
- Use `pulldown-cmark` events for markdown headings and wiki-link scanning in text events.
- Add `mime_guess` for cheap extension-to-MIME classification.
- Add `content_inspector` to avoid treating binary files as text.
- Consider `infer` for magic-number file type detection when the plugin previews non-source assets.
- Replace `serde_yaml`, which is deprecated, with a maintained YAML crate after evaluation. Prefer evaluating `serde_norway` first; `serde_yaml_ng` is another option.
- Use `blake3` for stable content-addressed snapshot hashes instead of `DefaultHasher`.
- Use `notify-debouncer-mini` to debounce raw watcher events.
- Consider `camino` for explicit UTF-8 repo-relative path invariants.

Important distinction:

- MIME/file-type detection should decide whether a file is text-like, previewable, or binary.
- Tree-sitter language selection should remain grammar/config driven, not MIME driven.

### 2. Tree-sitter Source Graph

Replace the conservative parser in `src/source_graph.rs` with tree-sitter-backed extraction.

Target behavior:

- Parse Rust, TypeScript, JavaScript, Python, and Go with first-class grammars.
- Extract modules, functions, methods, classes, imports, containment, and calls.
- Track symbol ranges, not just starting lines.
- Resolve method/function calls more accurately where practical.
- Keep the public `SourceGraph` API stable for `query` and `state`.

### 3. ast-grep Search and Enforcement

Replace lexical pattern fallback with `ast-grep-core`.

Target behavior:

- Compile TOML patterns and ADR `policy.patterns`.
- Run `criv search '<pattern>'`, `--pattern-id`, and `--rule` structurally.
- Return file/range/captures.
- Store pattern match lists in `.criv/state.json`.
- Make accepted ADR policy violations fail `criv check` and `criv enforce`.

### 4. fff-search Source Index

Add the real source index behind an internal trait.

Target behavior:

- Fuzzy file search with frecency.
- Partial path resolution backed by the source index.
- Grep modes over source.
- Source-side watcher integration so `criv watch` does not duplicate filesystem/indexing work.

### 5. Snapshot and Git Diff

Extend local snapshots into spec-level graph diffing.

Target behavior:

- Content-address nodes and edges.
- Store stable Merkle-like graph roots.
- Resolve `criv query diff <ref-a> <ref-b>` from git refs, not only snapshot hashes.
- Report node/edge additions/removals in text and JSON.

### 6. Note Retrieval

Implement vault note retrieval beyond substring search.

Target behavior:

- In-memory lexical index with ranking.
- Optional `fastembed` semantic search.
- Respect `index.embeddings` and binary-size tradeoffs.
- Return ranked note results with IDs, titles, paths, and excerpts.

### 7. Obsidian Plugin

Build the real plugin features on top of the sample template and WASM helper.

Target behavior:

- Read `.criv/state.json` and validate schema version.
- Resolve source wiki-links and show hover previews.
- Render pattern match lists from state.
- Provide partial-path autocomplete from source index state.
- Show drift markers for unresolved references.
- Keep source editing outside Obsidian.

Current implementation reality:

- The plugin reads `.criv/state.json`, validates the schema, and displays source
  previews and frontmatter pattern status.
- Source previews use explicit token-based syntax highlighting in hover previews
  and the source panel.
- Pattern rendering exposes per-match file, range, and capture details from
  `.criv/state.json`.
- Source autocomplete ranks by match quality and frecency over `source-index`,
  including partial-path and basename matches.
- Drift indicators exist for reading-mode links, pattern rows, and unresolved
  source or `match:` links while editing through a CodeMirror extension.
- The shared link-resolution fixture is installed into the plugin scaffold and
  covered by TypeScript tests.

### 8. Enforcement Integrations

Implement policy compilation and stage-specific checks.

Target behavior:

- `commit`: cheap import checks on staged files.
- `push`: graph-aware checks and changed-file pattern evaluation.
- `ci`: full `criv check` plus full enforcement.
- Invoke ESLint/Ruff where available and degrade gracefully when absent.
- Keep ast-grep pattern enforcement in-process.

## Release Policy

Use `cargo-release` for workspace releases.

Current tags:

- `v0.3.0`
- `v0.2.0`
- `v0.1.1`
- `v0.1.0`
- `criv-wasm-v0.3.0`
- `criv-wasm-v0.2.0`
- `criv-wasm-v0.1.1`
- `criv-wasm-v0.1.0`

Release automation target:

- Maintain a GitHub Actions workflow that builds `criv` release binaries only when a `v*` release tag is pushed.
- Build Linux assets on amd64 and arm64, macOS on Apple Silicon, and Windows on
  amd64.
- Archive each binary with hk-style Rust target-triple names so installers can select by platform.
- Include checksums and GitHub build provenance attestations for public release assets.
- Keep `criv --version` working as the installer smoke test.
- Use the release assets as the foundation for future aqua and mise registry entries.

Before the next release:

- Run `cargo test --workspace`.
- Run `cargo fmt --check`.
- Run `target/debug/criv check`.
- Run `target/debug/criv enforce --stage ci`.
- Run `mise run check`.
- Run `mise run perf` on this repository, and on a larger vault before changes
  that affect parsing, indexing, snapshot writing, or pattern matching.
- Keep this release git-tag-only. Crates.io publishing remains deferred until
  the CLI API, state schema compatibility policy, and installer story are stable.

## Next Implementation Steps

1. Keep adding regression fixtures when bugs are found.
   The current plugin resolver fixture covers source links, pattern links,
   autocomplete ranking, pattern match exposure, and editor drift ranges.
2. Use the performance script before shared backend changes.
   `mise run perf` records cold/warm watch timing, source-index startup,
   validation, enforcement, and diff timings for this or a larger vault.

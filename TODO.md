# TODO

## Done

- [x] Scaffold Rust CLI workspace.
- [x] Add `criv init`.
- [x] Add `criv check`.
- [x] Add real TOML config parsing.
- [x] Add real YAML frontmatter parsing.
- [x] Add note, ADR, wiki-link, target, and supersession validation.
- [x] Add initial query commands.
- [x] Add lexical source grep, file search, and note search.
- [x] Add `watch --once` state writing.
- [x] Add `.criv/state.json`.
- [x] Add local snapshots under `.criv/snapshots/`.
- [x] Add `criv query diff` for local snapshot hashes.
- [x] Add source graph fallback extraction.
- [x] Add callers/callees/attack-surface queries.
- [x] Add stage-aware `criv enforce`.
- [x] Add lexical fallback for structural search and ADR policy enforcement.
- [x] Add Obsidian sample-plugin-style scaffold.
- [x] Add Rust WASM helper crate for plugin-side logic.
- [x] Cut initial `0.1.0` release tags with `cargo-release`.
- [x] Replace hand-rolled CLI parsing with `clap` derive.
- [x] Replace custom `CrivError` internals with `thiserror`.
- [x] Replace `util::glob_matches` internals with `globset`.
- [x] Compile configured source exclude globs into a reusable `GlobSet`.
- [x] Use `pulldown-cmark` events for markdown headings.
- [x] Use `pulldown-cmark` events to avoid scanning wiki-links inside code.
- [x] Add `mime_guess` for cheap extension-to-MIME classification in state.
- [x] Add `content_inspector` to skip binary files during source indexing/search.
- [x] Replace deprecated `serde_yaml` with `serde_norway`.
- [x] Replace `DefaultHasher` snapshot hashing with `blake3`.
- [x] Add `notify-debouncer-mini` for watcher event debouncing.
- [x] Add note lexical index with ranking.
- [x] Add content-addressed node and edge hashes.
- [x] Add graph root hashing.
- [x] Add JSON output for all query/search/check variants.
- [x] Add git-ref support to `criv query diff <ref-a> <ref-b>`.
- [x] Make `criv check` fail accepted ADR policy violations.
- [x] Add `query coverage --by module`.
- [x] Add `query coverage --by adr`.
- [x] Implement single-instance watch lock.
- [x] Keep source-language detection grammar/config driven rather than MIME driven.
- [x] Add CI-oriented full enforcement mode.
- [x] Add stage-specific changed-file detection for commit/push.
- [x] Add Obsidian schema-version check.
- [x] Add `query nodes --kind code --without-docs` against precise symbols.
- [x] Add release automation docs for crates.io publishing.
- [x] Add ESLint integration for JS/TS import policies.
- [x] Add Ruff integration for Python import policies.
- [x] Add graceful missing-tool diagnostics for ESLint/Ruff.
- [x] Extract reliable containment edges for modules/classes/methods.
- [x] Add attack-surface semantics beyond uncalled symbols.
- [x] Improve call resolution beyond name matching.
- [x] Evaluate `miette` for validation diagnostics with source spans.
- [x] Evaluate `infer` for magic-number detection for plugin previews and non-source assets.
- [x] Evaluate `serde_yaml_ng` as an alternate YAML replacement.
- [x] Evaluate `camino` for UTF-8 repo-relative path handling.
- [x] Add native import-policy checks.
- [x] Replace fallback source parser with tree-sitter.
- [x] Add tree-sitter grammars for Rust.
- [x] Add tree-sitter grammars for TypeScript.
- [x] Add tree-sitter grammars for JavaScript.
- [x] Add tree-sitter grammars for Python.
- [x] Add tree-sitter grammars for Go.
- [x] Extract precise symbol ranges.

## Missing For Spec Completeness

### Structural Search And Enforcement

- [ ] Add `ast-grep-core`.
- [ ] Compile TOML patterns as ast-grep rules.
- [ ] Compile ADR `policy.patterns` as ast-grep rules.
- [ ] Store ast-grep match ranges and captures in `.criv/state.json`.
- [ ] Make `criv search '<pattern>'` structural.
- [ ] Make `criv search --pattern-id <id>` structural.
- [ ] Make `criv search --rule <ADR-ID>` structural.

### Source Index

- [ ] Add `fff-search` behind a `SourceIndex` trait.
- [ ] Use fff for fuzzy file search.
- [ ] Use fff for grep modes.
- [ ] Use fff for partial-path reference resolution.
- [ ] Use fff watcher events in `criv watch`.
- [ ] Add incremental reparsing.
- [ ] Add incremental pattern match updates.

### Note Retrieval

- [ ] Add optional `fastembed` semantic note search.

### Obsidian Plugin

- [x] Add Obsidian source-reference hover previews.
- [x] Add Obsidian source-reference side panel.
- [x] Add Obsidian external editor URL support.
- [x] Add Obsidian pattern-reference rendering.
- [ ] Add Obsidian frontmatter pattern target rendering.
- [x] Add Obsidian drift indicators.
- [x] Add Obsidian partial-path autocomplete.
- [ ] Share link-resolution fixtures between CLI and plugin.

### Build And Release

- [ ] Build and check the plugin with `npm run build`.
- [ ] Build WASM package with `wasm-pack`.

## Current Verification Commands

```sh
cargo test --workspace
cargo fmt --check
target/debug/criv check
target/debug/criv enforce --stage ci
target/debug/criv watch --once
target/debug/criv query diff latest latest
```

## Notes

- Current structural search is a lexical fallback, not real ast-grep.
- Current source graph extraction is tree-sitter-backed with a conservative fallback.
- Current Obsidian plugin reads state and uses a WASM helper, but does not yet render code previews or match lists.
- `.criv/` is local state and intentionally ignored by git.

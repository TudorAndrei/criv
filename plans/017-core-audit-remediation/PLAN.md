# Plan: Core audit remediation

## Goal

Resolve all eleven findings from the 2026-07-23 core implementation audit at
commit `70fc84c`: make cached source analysis correct, make Git enforcement
complete and fail-closed, confine generated writes, validate and accelerate
policy matching, fix cross-platform parsing and JSX filtering, stabilize the
state contract, remove redundant state work, cover the real watch loop,
consolidate source indexing, and refresh the recorded dependency posture.

## Approach

Land the work as eleven independently reviewable phases. Each phase corresponds
to one audited finding and ends in a conventional commit with targeted tests.
The high-impact safety and governance fixes land first. Characterization tests
land before the state and watch refactors they protect, and the source-index
consolidation lands after graph-cache correctness and watch-loop coverage.

The implementation should preserve the public CLI and
`criv.state.v0` contracts unless a phase explicitly describes a new validation
error. In particular:

- Source cache reuse must be content-correct even when size and mtime collide.
- Git discovery errors must not silently disable ADR immutability.
- Repository-controlled paths must not cause writes outside their allowed
  directory through symlinks.
- Invalid configured globs must fail during configuration loading.
- State optimization must not change serialized bytes, snapshot hashes, or
  consumer behavior.
- Source-index consolidation must preserve ignore, binary-file, explicit-file,
  frecency, fuzzy search, grep, and path-resolution semantics.

Accepted ADRs are immutable. If Phase 1's output-confinement decision is judged
to change the contract established by ADR-0029/ADR-0030, add a new ADR and
update `docs/adr/README.md`; do not edit either accepted ADR. Product-direction
options from the audit (`query diff` modifications, CI annotations, embeddings
distribution, and editor installation) are outside this remediation plan.

### Finding coverage

| Audit finding | Phase |
| --- | --- |
| 1. Stale source-graph cache reuse | 3 |
| 2. Incomplete/fail-open Git enforcement | 2 |
| 3. Symlink-escaping and non-atomic writes | 1 |
| 4. Invalid and repeatedly compiled policy globs | 4 |
| 5. Overlapping source scans and indexes | 10 |
| 6. CRLF frontmatter rejection | 5 |
| 7. JSX language filter mismatch | 6 |
| 8. Redundant state materialization | 8 |
| 9. No shared state-contract fixture | 7 |
| 10. Long-running watch loop untested | 9 |
| 11. Stale dependency posture record | 11 |

## Implementation Phases

### Phase 1: Confine and atomically publish generated writes

- Add a root-aware write-path validator in `src/util.rs`. Resolve the vault
  root and the nearest existing ancestor of the destination, reject symlinked
  path components, and prove the final destination is inside the allowed
  subtree before creating directories or temporary files.
- Keep `write_atomic` as the final publication mechanism. Extend or wrap it so
  callers cannot validate one path and write another.
- In `src/config.rs::RawArchitectureCode::into_config`, retain lexical
  rejection of absolute and parent-directory paths. At runtime, require
  `architecture.code.output` to resolve beneath the configured docs directory,
  matching ADR-0029/ADR-0030's “ordinary vault content” contract.
- Route `src/architecture.rs::write_code_architecture_with_config` through the
  confined atomic writer. Reject an output directory or file symlink instead of
  following it.
- Route `src/check.rs::apply_markdown_fixes` through atomic replacement and
  verify its already-discovered note path remains inside the configured docs
  tree.
- Protect internal `.criv` writes in `src/state.rs`,
  `src/source_graph.rs`, and `src/watch.rs::WatchLock::acquire` by rejecting a
  symlinked `.criv` directory or symlinked destination. Preserve atomic state,
  cache, snapshot, and `latest` publication.
- Add unit tests in `src/util.rs` and CLI tests in
  `tests/cli_workflows.rs` for a symlinked architecture parent, symlinked
  `.criv`, unchanged-output no-op, nested new directories, and successful
  atomic replacement.
- If output confinement requires a new decision, create the next ADR under
  `docs/adr/`, govern the write-path call sites, and add it to
  `docs/adr/README.md`.
- Run `cargo test --workspace`, the architecture/watch CLI tests, and
  `cargo run --quiet -- check`.

**Commit:** `fix(io): confine and atomically publish generated writes`

### Phase 2: Make Git change discovery complete and fail-closed

- Extend `src/enforce.rs::EnforceOptions` with an explicit generated-hook mode
  for pre-push input. Update `src/init/templates.rs::pre_push_hook` to pass the
  hook's remote name/location and let `criv enforce --stage push` consume the
  ref-update lines from stdin without making manual invocations block.
- Parse pre-push records as
  `<local-ref> <local-oid> <remote-ref> <remote-oid>`, including multiple
  updates, branch creation/deletion, and all-zero object IDs.
- For existing remote refs, enumerate every outgoing commit in
  `remote_oid..local_oid`. For new branches, enumerate commits not already
  reachable from the named remote. Inspect each outgoing commit's name-status
  changes so an ADR added and then modified within one first push is not
  flattened into a final “added” result.
- Replace newline/tab parsing in `git_changed_entries` with Git `-z`
  name-status output and byte-safe field parsing. Preserve rename/copy
  old/new paths; reject paths that cannot be represented by criv's UTF-8 path
  model with a precise error.
- Change Git command helpers to return `Result` with command, status, and
  stderr context. The commit, push, and CI immutability gates must fail closed
  when the required comparison cannot be computed.
- Preserve the documented non-CI fallback only for deliberate manual
  `enforce --stage push` runs. Print which comparison basis was used so “zero
  changed files” is distinguishable from failed discovery.
- Keep policy/native-tool changed-file scoping and
  `is_allowed_adr_link_migration` behavior intact while supplying accurate old
  and new refs for each change.
- Add `src/enforce.rs` unit tests for NUL-delimited add/modify/delete/rename/copy
  records and `tests/cli_workflows.rs` repositories covering first push,
  multiple outgoing commits, multiple ref updates, missing Git, invalid refs,
  and an ordinary upstream push.
- Update generated-hook assertions in `src/init/tests.rs`.
- Run `cargo test --workspace`, targeted enforcement CLI tests,
  `cargo run --quiet -- enforce --stage ci`, and
  `cargo run --quiet -- check`.

**Commit:** `fix(enforce): derive complete fail-closed Git change sets`

### Phase 3: Make source-graph cache identity content-correct

- Replace `source_file_fingerprint` in `src/source_graph.rs` with a versioned
  content digest. BLAKE3 the bytes that tree-sitter/fallback parsing consumes;
  do not rely on size, mtime, or fff metadata for correctness.
- Refactor `SourceGraph::build_incremental` so each file is read at most once
  per build: compute the digest, reuse the cached `SourceFile` when it matches,
  and pass the already-read contents to `parse_source_file` when it differs.
- Bump `GRAPH_CACHE_SCHEMA` because the meaning of
  `file_fingerprints` changes. Continue treating missing, corrupt, or
  wrong-schema caches as cold-cache misses.
- Write `.criv/source-graph.json` only when the graph/cache representation
  changed. Preserve deterministic serialization and atomic publication from
  Phase 1.
- Change `Vault::load` and the `watch --once` startup path to hydrate from the
  persisted cache consistently. Keep the in-memory previous graph for
  long-running incremental rebuilds.
- Add regression tests that rewrite a source file to same-length content,
  restore its mtime, and verify updated symbols/imports/calls are returned.
  Cover deletion, cache corruption, schema mismatch, and unchanged-cache mtime.
- Record `mise run perf` before and after. Treat correctness as mandatory; if
  hashing regresses large-file performance, optimize shared reads rather than
  returning to metadata-only identity.
- Run `cargo test --workspace`, graph/cache CLI tests,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo run --quiet -- check`, and `mise run perf`.

**Commit:** `fix(graph): key incremental cache reuse by source content`

### Phase 4: Validate and compile policy matchers once

- Audit every config field consumed through `glob_matches`, starting with
  `RawImportPolicy.scope`, glob-shaped `RawImportPolicy.deny` entries, global
  excludes, source excludes, ADR governs scopes, and configured pattern paths.
- Make `RawImportPolicy::into_policy` return `Result<ImportPolicy>` and validate
  its scope and wildcard deny expressions during `Config::load`. Report the
  field/policy ID and invalid expression; never convert matcher construction
  failure into “not matched.”
- Keep `glob_matches` only for trusted one-off expressions. Use
  `GlobMatcher` values compiled once for repeated config/vault operations.
- In `check.rs::policy_scope_files` and
  `enforce.rs::policy_scope_files`, preserve scope expansion and sorted
  deduplication. In `structural.rs::PolicyScanRequest`, represent the expanded
  paths as exact membership sets instead of recompiling each path as a glob
  inside `path_allowed`.
- Ensure the batch scanner still reads/parses each source file once and runs
  only same-language requests, preserving result ordering and error semantics.
- Add configuration tests for malformed scopes and deny globs, plus a CLI
  regression where an invalid import-policy scope must fail before enforcement.
- Add structural tests proving exact path membership, overlapping scopes, empty
  scopes, changed-file intersections, and byte-identical violation ordering.
- Extend `scripts/measure-performance.sh` only if necessary to create a
  repeatable many-policies/many-files probe; otherwise record a separate
  scratch-vault benchmark in the implementation notes.
- Run `cargo test --workspace`, `cargo run --quiet -- check`,
  `cargo run --quiet -- enforce --stage ci`, and `mise run perf`.

**Commit:** `fix(policy): validate and reuse compiled scope matchers`

### Phase 5: Parse frontmatter delimiters across line endings

- Replace `vault.rs::split_frontmatter`'s prefix/search implementation with a
  delimiter-line parser that accepts `---` followed by LF or CRLF at the start
  and requires the closing delimiter line to contain exactly `---`.
- Preserve frontmatter/body byte content as far as YAML and Markdown consumers
  permit; do not globally normalize note line endings or move diagnostic line
  numbers.
- Treat missing closing delimiters as body-only content, preserving current
  validation behavior unless tests document a more precise existing contract.
- Add `src/vault.rs` tests for LF, CRLF, mixed line endings, BOM/no-BOM behavior,
  `---suffix`, delimiter-like body text, empty frontmatter, and missing close.
- Add a CLI regression showing a valid CRLF note passes schema validation.
- Run `cargo test --workspace` and `cargo run --quiet -- check`.

**Commit:** `fix(vault): recognize exact frontmatter delimiters with CRLF`

### Phase 6: Correct JSX language filtering

- Update `structural.rs::language_glob` so `jsx` selects `.jsx`, while
  `javascript`/`js` retain their documented `.js` behavior unless existing CLI
  help explicitly defines them as a JavaScript-family filter.
- If one language needs multiple extensions, replace the single `&str` return
  with a small matcher/list and update
  `state.rs::configured_pattern_paths` and search filtering together.
- Verify `SupportLang::from_path` and ast-grep language parsing agree for
  `.js`, `.jsx`, `.ts`, and `.tsx`.
- Add structural/state unit tests and a `tests/cli_workflows.rs` regression for
  `search --files main --lang jsx`.
- Run `cargo test --workspace` and `cargo run --quiet -- check`.

**Commit:** `fix(search): match JSX files for the JSX language filter`

### Phase 7: Pin the shared `criv.state.v0` producer/consumer contract

- Add a checked-in canonical fixture under `fixtures/state/` containing the
  complete `criv.state.v0` envelope: graph root, representative nodes/edges and
  hashes, registered patterns, pattern matches, and source-index entries.
- In `src/state.rs`, construct the canonical state from a deterministic temp
  vault and compare its serialized JSON value with the fixture. Keep the
  producer authoritative; fixture updates require an explicit schema-contract
  review.
- Replace the hand-written minimal JSON in
  `crates/criv-wasm/src/lib.rs::tests::parses_state_shape` with the shared
  fixture and assert the WASM summaries, graph nodes, source entries, and
  registered patterns derived from it.
- Add fixture-driven tests to the Obsidian companion and
  `extensions/vscode-criv` state-model suites. Resolve the repository fixture
  through test configuration rather than copying JSON into each package.
- Add a negative wrong-schema fixture or generated mutation and verify every
  consumer rejects it consistently.
- Do not change `STATE_SCHEMA` or make optional fields required in this phase.
- Run `cargo test --workspace`,
  `npm --prefix .obsidian/plugins/criv test`,
  `npm --prefix extensions/vscode-criv test`, and
  `cargo run --quiet -- check`.

**Commit:** `test(state): share a golden v0 contract across consumers`

### Phase 8: Serialize and build state once per rebuild

- Refactor `State::write`, `State::hash`, and `State::write_snapshot` in
  `src/state.rs` so `write_state` and `write_state_incremental` serialize the
  state once, compute the hash from those exact bytes, and reuse the bytes for
  `state.json` and a newly created snapshot.
- Preserve the current pretty JSON plus trailing-newline bytes and therefore
  preserve snapshot IDs for identical state. Add an assertion against the
  Phase 7 golden fixture and a before/after hash regression.
- Refactor `watch.rs::rebuild` to return the `State` produced by
  `state::write_state` rather than immediately calling `State::build` again.
  Keep the architecture-write/reload ordering and previous-state C4-interface
  validation semantics.
- Avoid cloning the full source graph/state where ownership can move without
  complicating the loop; retain clones when they make failure recovery safer.
- Add unit tests or test-only counters proving one serialization and one state
  build per successful rebuild, without timing-based assertions.
- Record `mise run perf` before and after and confirm `diff latest latest`
  remains empty.
- Run `cargo test --workspace`, state/watch tests,
  `cargo run --quiet -- watch --once`,
  `cargo run --quiet -- query diff latest latest`,
  `cargo run --quiet -- check`, and `mise run perf`.

**Commit:** `perf(state): reuse one serialized state per rebuild`

### Phase 9: Exercise the long-running watch event loop

- Extract the event classification/rebuild decision logic from
  `watch.rs::run` into a deterministic helper that can be unit-tested without
  sleeping. Cover docs-only, source-only, simultaneous, timeout, watcher error,
  and disconnected-channel cases.
- Add a CLI integration harness in `tests/cli_workflows.rs` that spawns
  `criv watch`, waits for `criv watch running`, edits a note, edits a source
  file, and polls `.criv/state.json` for both updates.
- Ensure every spawned child is terminated and waited on even after assertion
  failure. Use bounded polling with diagnostic output rather than fixed long
  sleeps.
- Verify failed rebuilds leave the last good state readable and a subsequent
  valid event recovers.
- Verify debounced bursts converge to one correct final state without requiring
  an exact notification count.
- Retain the existing lock-before-rebuild and `watch --once` coverage.
- Run `cargo test --workspace`, repeat the new watch integration test several
  times, and `cargo run --quiet -- check`.

**Commit:** `test(watch): cover event-driven incremental rebuilds`

### Phase 10: Consolidate source enumeration and watch indexing

- Make `FffSourceIndex` in `src/source_index.rs` the authoritative source-file
  enumerator when source indexing is enabled. Remove
  `vault.rs::collect_source_files` only after parity tests cover source roots,
  explicit file roots, excludes, Git ignores, hidden files, binary detection,
  duplicates, and stable ordering.
- Introduce an ownership model that lets `Vault` and long-running watch share
  one initialized fff index/picker state. Prefer an injected/shared
  `SourceIndex` over constructing a second watcher after `rebuild`.
- Adjust `Vault::load`/`load_incremental` so ordinary one-shot commands create
  one non-watching index, while `watch::run` creates one watch-enabled index and
  passes it through subsequent vault rebuilds.
- Keep persisted `SourceGraph` hydration from Phase 3 separate from live fff
  picker state: the graph cache is durable derived data; fff remains the
  current file/search/frecency authority.
- Replace the 250 ms whole-index fingerprint poll only if fff exposes a
  reliable change signal in the pinned API. Otherwise retain polling over the
  shared watcher and document why.
- Preserve `Vault::source_files`, `Vault::source_index`, fuzzy file search,
  grep, partial-path resolution, frecency entries, and deterministic state
  output.
- Add source-index/vault parity tests and use the Phase 9 integration harness
  to prove repeated source additions, modifications, renames, and deletions.
- Record cold/warm `mise run perf` before and after. Require no correctness
  regression even if the small-repository timing is noisy.
- Run `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo run --quiet -- check`, and `mise run perf`.

**Commit:** `refactor(index): share one source catalog with watch`

### Phase 11: Refresh dependency advisory posture

- Re-run `cargo audit --no-fetch` with the repository's pinned tool and record
  the advisory IDs, affected versions, dependency paths, and audit-database
  date in `docs/dependency-evaluations.md`.
- Correct the stale fff-search version and remove the obsolete statement that
  criv directly depends on git2.
- Inspect the current fff-search source for the affected git2 APIs and document
  whether `Remote::list` or buffer-created blame hunks are reachable. Keep this
  as evidence, not a claim that absence from text proves absence at runtime.
- Re-evaluate the existing monitor-only decision for `git2`, `bincode`, and
  `paste`. If the decision changes, create a new ADR rather than rewriting an
  accepted ADR.
- Make the document distinguish vulnerabilities, unsound-but-unreached APIs,
  unmaintained crates, inactive/optional lockfile entries, and local advisory
  database limitations.
- Do not add a failing audit gate or replace fff-search in this phase unless a
  new decision explicitly authorizes that expansion.
- Run `cargo tree -i git2@0.20.4`, `cargo tree -i bincode@1.3.3`,
  target/feature-specific tree checks for `paste`, `cargo audit --no-fetch`,
  and `cargo run --quiet -- check`.

**Commit:** `docs(deps): refresh transitive advisory posture`

## Risks & Tradeoffs

- Content hashing makes cache reuse correct but requires reading every indexed
  source file. Reuse the bytes and avoid duplicate reads; do not weaken the
  identity back to metadata.
- Root-aware safe writes may reject repositories that intentionally symlink
  `.criv` or generated docs. Security takes precedence, but errors must name the
  rejected component and migration path.
- Full pre-push commit enumeration costs more than one aggregate diff and may
  encounter shallow history. Fail with a precise fetch/base-ref instruction
  rather than silently weakening immutability.
- Rejecting invalid globs changes fail-open configurations into startup errors.
  Include the policy ID and field so remediation is straightforward.
- Actual filesystem watcher tests can be flaky across operating systems. Use
  bounded polling, deterministic helper tests, and strict child cleanup.
- Sharing fff picker state increases lifetime/ownership complexity. Keep the
  durable graph cache and live picker responsibilities distinct.
- The golden state fixture becomes an intentional compatibility gate. Additive
  producer changes will require an explicit fixture review even when the schema
  string stays `v0`.
- The eleven commits touch adjacent core modules. Re-run drift checks before
  each phase and update this plan/TODO if implementation needs to split a phase.

## Open Questions

- Should `architecture.code.output` be strictly beneath the configured docs
  directory, as recommended by the “ordinary vault content” ADR language, or
  may it target any non-symlinked path beneath the vault root? Resolve before
  Phase 1 implementation.
- For manual `enforce --stage push`, should missing upstream remain a documented
  full-local-history fallback, or should complete push enforcement be available
  only through generated-hook stdin? Generated hooks must be complete either
  way.
- Should `javascript`/`js` include both `.js` and `.jsx`, or should only the
  explicit `jsx` selector include `.jsx`? Preserve documented behavior and add
  explicit help/tests.
- If current fff-search cannot share watcher state cleanly, is a narrow upstream
  contribution acceptable, or should Phase 10 stop after eliminating the
  duplicate non-fff walk? Do not add a second indexing dependency.

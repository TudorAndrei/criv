# TODO: Core audit remediation

## Phase 1: Confine and atomically publish generated writes

- [x] Add and test a root-aware, symlink-rejecting write-path validator in
  `src/util.rs`.
- [x] Confine `architecture.code.output` to the chosen allowed subtree and use
  atomic publication in `src/architecture.rs`.
- [x] Make `check --fix` publish Markdown changes atomically inside docs.
- [x] Reject symlinked `.criv` paths for state, graph-cache, snapshot, latest,
  and watch-lock writes.
- [x] Add architecture-output, `.criv`, new-directory, no-op, and replacement
  regressions.
- [x] No new ADR: ADR-0029/ADR-0030 already require generated architecture to
  remain ordinary vault content, so restricting output to `vault.docs` enforces
  the accepted contract.
- [x] Run targeted tests, `cargo test --workspace`, and
  `cargo run --quiet -- check`.
- [x] Commit: `fix(io): confine and atomically publish generated writes`

## Phase 2: Make Git change discovery complete and fail-closed

- [x] Add explicit pre-push hook mode/arguments to `EnforceOptions` and
  `pre_push_hook`.
- [x] Parse every pre-push ref update, including new/deleted branches and
  multiple updates.
- [x] Enumerate outgoing commits and preserve per-commit ADR changes on first
  push.
- [x] Switch Git name-status transport to `-z` and parse rename/copy records
  without line trimming.
- [x] Return contextual errors from Git helpers and fail closed when
  immutability comparisons are unavailable.
- [x] Preserve changed-file policy/native-tool scoping and link-migration
  allowances.
- [x] Add parser, first-push, multi-commit, multi-ref, Git-error, and upstream
  regressions.
- [x] Update generated-hook tests in `src/init/tests.rs`.
- [x] Run targeted tests, `cargo test --workspace`,
  `cargo run --quiet -- enforce --stage ci`, and
  `cargo run --quiet -- check`.
- [x] Commit: `fix(enforce): derive complete fail-closed Git change sets`

## Phase 3: Make source-graph cache identity content-correct

- [x] Replace size/mtime fingerprints with BLAKE3 content digests.
- [x] Read each source file once while deciding reuse versus parse.
- [x] Bump `GRAPH_CACHE_SCHEMA` and retain cold fallback for bad caches.
- [x] Skip cache publication when serialized cache content is unchanged.
- [x] Hydrate one-shot and watch startup consistently from the persisted graph.
- [x] Add same-size/restored-mtime, deletion, corruption, schema, and unchanged
  cache regressions.
- [x] Record before/after `mise run perf`: baseline `a3aa1ce` reported
  `watch_once_warm=1.60s`; the Phase 3 worktree reported `1.30s` (cold:
  `1.87s` → `1.42s`; small-repository timings are informational).
- [x] Run workspace tests, clippy, self-check, and performance verification.
- [x] Commit: `fix(graph): key incremental cache reuse by source content`

## Phase 4: Validate and compile policy matchers once

- [x] Inventory config/ADR globs consumed through `glob_matches`.
- [x] Make import-policy conversion validate scopes and wildcard denies.
- [x] Return precise configuration errors instead of false non-matches.
- [x] Reuse compiled `GlobMatcher` values for repeated matching.
- [x] Use exact set membership for already-expanded policy paths in the batch
  scanner.
- [x] Preserve one parse per file, ordering, language, and changed-file
  semantics.
- [x] Add invalid-glob configuration tests and scope/membership/ordering unit tests.
- [x] Benchmark with the repository performance probe; no dedicated fixture was
  needed because the exact-membership change preserves the existing one-parse
  batch scanner behavior.
- [x] Run workspace tests, check, CI enforcement, and performance verification.
- [x] Commit: `fix(policy): validate and reuse compiled scope matchers`

## Phase 5: Parse frontmatter delimiters across line endings

- [x] Replace prefix searching with exact delimiter-line parsing.
- [x] Support LF and CRLF without globally normalizing note content.
- [x] Reject `---suffix` as a closing delimiter.
- [x] Add LF, CRLF, mixed, BOM, delimiter-like body, empty, and unclosed tests.
- [x] Add a valid-CRLF CLI validation regression.
- [x] Run `cargo test --workspace` and `cargo run --quiet -- check`.
- [x] Commit: `fix(vault): recognize exact frontmatter delimiters with CRLF`

## Phase 6: Correct JSX language filtering

- [ ] Make `jsx` match `.jsx` paths.
- [ ] Resolve and document whether `javascript`/`js` also include `.jsx`.
- [ ] Keep `.js`, `.jsx`, `.ts`, and `.tsx` path/language parsing consistent.
- [ ] Add structural/state unit tests and a JSX CLI search regression.
- [ ] Run `cargo test --workspace` and `cargo run --quiet -- check`.
- [ ] Commit: `fix(search): match JSX files for the JSX language filter`

## Phase 7: Pin the shared `criv.state.v0` producer/consumer contract

- [ ] Add the complete canonical fixture under `fixtures/state/`.
- [ ] Compare deterministic Rust producer output to the fixture.
- [ ] Drive criv-wasm state tests from the same fixture.
- [ ] Drive Obsidian and VS Code state-model tests from the same fixture.
- [ ] Verify every consumer consistently rejects a wrong schema.
- [ ] Confirm `STATE_SCHEMA` and public optionality are unchanged.
- [ ] Run Rust, Obsidian, VS Code, and vault verification commands.
- [ ] Commit: `test(state): share a golden v0 contract across consumers`

## Phase 8: Serialize and build state once per rebuild

- [ ] Serialize state once and reuse the exact bytes for hash, state, and
  snapshot publication.
- [ ] Preserve pretty JSON, trailing newline, and snapshot hashes.
- [ ] Return/reuse the built `State` from watch rebuild startup.
- [ ] Preserve architecture reload and C4-interface validation ordering.
- [ ] Add non-timing assertions for one state build/serialization per rebuild.
- [ ] Confirm `query diff latest latest` stays empty.
- [ ] Record before/after `mise run perf`.
- [ ] Run workspace, state/watch, check, diff, and performance verification.
- [ ] Commit: `perf(state): reuse one serialized state per rebuild`

## Phase 9: Exercise the long-running watch event loop

- [ ] Extract deterministic event/rebuild decision logic.
- [ ] Unit-test docs/source/simultaneous/timeout/error/disconnect decisions.
- [ ] Add a bounded CLI harness for real note and source events.
- [ ] Guarantee spawned watcher termination and wait on every path.
- [ ] Verify recovery after a failed rebuild and valid follow-up event.
- [ ] Verify debounced bursts converge to correct final state.
- [ ] Repeat the integration test to detect flakiness.
- [ ] Run `cargo test --workspace` and `cargo run --quiet -- check`.
- [ ] Commit: `test(watch): cover event-driven incremental rebuilds`

## Phase 10: Consolidate source enumeration and watch indexing

- [ ] Add parity tests for roots, explicit files, excludes, ignores, hidden
  files, binaries, duplicates, and ordering.
- [ ] Make `FffSourceIndex` authoritative for enabled source-file enumeration.
- [ ] Remove `vault.rs::collect_source_files` after parity is proven.
- [ ] Share one watch-enabled fff index/picker lifecycle with `Vault` rebuilds.
- [ ] Keep durable graph cache and live picker responsibilities separate.
- [ ] Preserve search, grep, resolution, frecency, and state behavior.
- [ ] Exercise add/modify/rename/delete through the Phase 9 watch harness.
- [ ] Record cold/warm performance before and after.
- [ ] Run workspace tests, clippy, self-check, and performance verification.
- [ ] Commit: `refactor(index): share one source catalog with watch`

## Phase 11: Refresh dependency advisory posture

- [ ] Re-run the pinned local Cargo audit and record database date/advisory IDs.
- [ ] Correct current fff-search/git2 dependency paths and versions.
- [ ] Inspect and document reachability evidence for affected git2 APIs.
- [ ] Distinguish vulnerabilities, unsound APIs, unmaintained crates, and
  inactive optional lockfile entries.
- [ ] Re-evaluate the monitor-only decision without adding an unapproved gate or
  dependency replacement.
- [ ] Add a new ADR if the dependency policy decision changes.
- [ ] Run inverse dependency trees, target/feature tree checks, audit, and
  `cargo run --quiet -- check`.
- [ ] Commit: `docs(deps): refresh transitive advisory posture`

## Verification

- [ ] `mise run check` passes after every phase and at the final commit.
- [ ] `cargo test --workspace` passes after every Rust phase.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes after the
  final core refactors.
- [ ] `npm --prefix .obsidian/plugins/criv test` passes after the state-contract
  fixture lands.
- [ ] `npm --prefix extensions/vscode-criv test` passes after the state-contract
  fixture lands.
- [ ] `cargo run --quiet -- check` reports zero errors for this vault.
- [ ] `cargo run --quiet -- enforce --stage ci` passes with accurate Git
  comparison diagnostics.
- [ ] Manual smoke: `watch --once`, `check`, search, query, and enforce work on
  an initialized scratch vault.
- [ ] Security smoke: symlinked architecture and `.criv` paths fail without
  modifying the external targets.
- [ ] Git edge cases: first push, multiple commits, new/deleted refs, rename,
  unusual filenames, missing Git, and shallow/missing comparison history.
- [ ] Cache edge cases: same size/mtime, add/delete/rename, corrupt cache,
  wrong schema, and unchanged cache.
- [ ] Policy edge cases: invalid scope/deny globs, overlapping scopes, empty
  scopes, language mismatch, and changed-file intersections.
- [ ] Parsing/search edge cases: CRLF frontmatter and `.jsx` filtering.
- [ ] State fixture is consumed by Rust, WASM, Obsidian, and VS Code tests.
- [ ] Long-running watcher handles note/source changes, burst events, failed
  rebuild recovery, and clean termination.
- [ ] `mise run perf` before/after results are recorded for graph, policy,
  state, and source-index phases.
- [ ] `cargo audit --no-fetch` findings and limitations match
  `docs/dependency-evaluations.md`.
- [ ] No accepted ADR was edited; any changed decision is captured in a new ADR.
- [ ] `.criv/` remains ignored and no scratch fixtures are tracked.

## Review

- [ ] Code reviewed phase by phase.
- [ ] `PLAN.md` updated if scope, APIs, or phase boundaries changed.
- [ ] `TODO.md` checked only after the corresponding command or commit succeeds.
- [ ] Every phase commit is clean and uses the exact drafted conventional
  commit message.
- [ ] `plans/README.md` status for plan 017 is updated as work progresses.
- [ ] All TODO items are checked off before plan 017 is marked DONE.

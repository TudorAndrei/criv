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

- [x] Make `jsx` match `.jsx` paths.
- [x] Keep `javascript`/`js` scoped to `.js`; only explicit `jsx` includes `.jsx`.
- [x] Keep `.js`, `.jsx`, `.ts`, and `.tsx` path/language parsing consistent.
- [x] Add structural and JSX CLI search regressions.
- [x] Run `cargo test --workspace` and `cargo run --quiet -- check`.
- [x] Commit: `fix(search): match JSX files for the JSX language filter`

## Phase 7: Pin the shared `criv.state.v0` producer/consumer contract

- [x] Add the complete canonical fixture under `fixtures/state/`.
- [x] Compare deterministic Rust producer output to the fixture.
- [x] Drive criv-wasm state tests from the same fixture.
- [x] Drive Obsidian and VS Code state-model tests from the same fixture.
- [x] Verify every consumer consistently rejects a wrong schema.
- [x] Confirm `STATE_SCHEMA` and public optionality are unchanged.
- [x] Run Rust, Obsidian, VS Code, and vault verification commands.
- [x] Commit: `test(state): share a golden v0 contract across consumers`

## Phase 8: Serialize and build state once per rebuild

- [x] Serialize state once and reuse the exact bytes for hash, state, and
  snapshot publication.
- [x] Preserve pretty JSON, trailing newline, and snapshot hashes.
- [x] Return/reuse the built `State` from watch rebuild startup.
- [x] Preserve architecture reload and C4-interface validation ordering.
- [x] Add non-timing assertions for one state build/serialization per rebuild.
- [x] Confirm `query diff latest latest` stays empty.
- [x] Record before/after `mise run perf`: baseline cold/warm was `1.91s` /
  `1.41s`; after was `1.87s` / `1.62s`. This small-repository timing variance
  is informational; the test-only counters prove one state build and
  serialization per rebuild.
- [x] Run workspace, state/watch, check, diff, and performance verification.
- [x] Commit: `perf(state): reuse one serialized state per rebuild`

## Phase 9: Exercise the long-running watch event loop

- [x] Extract deterministic event/rebuild decision logic.
- [x] Unit-test docs/source/simultaneous/timeout/error/disconnect decisions.
- [x] Add a bounded CLI harness for real note and source events.
- [x] Guarantee spawned watcher termination and wait on every path.
- [x] Verify recovery after a failed rebuild and valid follow-up event.
- [x] Verify debounced bursts converge to correct final state.
- [x] Repeat the integration test to detect flakiness.
- [x] Run `cargo test --workspace` and `cargo run --quiet -- check`.
- [x] Commit: `test(watch): cover event-driven incremental rebuilds`

## Phase 10: Consolidate source enumeration and watch indexing

- [x] Add parity tests for roots, explicit files, excludes, ignores, hidden
  files, binaries, duplicates, and ordering.
- [x] Make `FffSourceIndex` authoritative for enabled source-file enumeration.
- [x] Remove `vault.rs::collect_source_files` after parity is proven.
- [x] Share one watch-enabled fff index/picker lifecycle with `Vault` rebuilds.
- [x] Keep durable graph cache and live picker responsibilities separate.
- [x] Preserve search, grep, resolution, frecency, and state behavior.
- [x] Exercise add/modify/rename/delete through the Phase 9 watch harness.
- [x] Record cold/warm performance before and after: the Phase 9 baseline was
  `1.76s`; the Phase 10 run was `3.14s` (cold-like direct runs). CPU time was
  comparable and fff scan wait dominated wall time, so this small-repository
  variance is recorded rather than treated as a correctness regression.
- [x] Run workspace tests, clippy, self-check, and performance verification.
- [x] Commit: `refactor(index): share one source catalog with watch`

## Phase 11: Refresh dependency advisory posture

- [x] Re-run the pinned local Cargo audit and record database date/advisory IDs.
- [x] Correct current fff-search/git2 dependency paths and versions.
- [x] Inspect and document reachability evidence for affected git2 APIs.
- [x] Distinguish vulnerabilities, unsound APIs, unmaintained crates, and
  inactive optional lockfile entries.
- [x] Re-evaluate the monitor-only decision without adding an unapproved gate or
  dependency replacement.
- [x] Add a new ADR if the dependency policy decision changes (not needed: the
  monitor-only policy is unchanged).
- [x] Run inverse dependency trees, target/feature tree checks, audit, and
  `cargo run --quiet -- check`.
- [x] Commit: `docs(deps): refresh transitive advisory posture`

## Verification

- [x] `mise run check` passes after every phase and at the final commit.
- [x] `cargo test --workspace` passes after every Rust phase.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes after the
  final core refactors.
- [x] `npm --prefix .obsidian/plugins/criv test` passes after the state-contract
  fixture lands.
- [x] `npm --prefix extensions/vscode-criv test` passes after the state-contract
  fixture lands.
- [x] `cargo run --quiet -- check` reports zero errors for this vault.
- [x] `cargo run --quiet -- enforce --stage ci` passes with accurate Git
  comparison diagnostics.
- [x] Manual smoke: `watch --once`, `check`, search, query, and enforce work on
  an initialized scratch vault.
- [x] Security smoke: symlinked architecture and `.criv` paths fail without
  modifying the external targets.
- [x] Git edge cases: first push, multiple commits, new/deleted refs, rename,
  unusual filenames, missing Git, and shallow/missing comparison history.
- [x] Cache edge cases: same size/mtime, add/delete/rename, corrupt cache,
  wrong schema, and unchanged cache.
- [x] Policy edge cases: invalid scope/deny globs, overlapping scopes, empty
  scopes, language mismatch, and changed-file intersections.
- [x] Parsing/search edge cases: CRLF frontmatter and `.jsx` filtering.
- [x] State fixture is consumed by Rust, WASM, Obsidian, and VS Code tests.
- [x] Long-running watcher handles note/source changes, burst events, failed
  rebuild recovery, and clean termination.
- [x] `mise run perf` before/after results are recorded for graph, policy,
  state, and source-index phases.
- [x] `cargo audit --no-fetch` findings and limitations match
  `docs/dependency-evaluations.md`.
- [x] No accepted ADR was edited; any changed decision is captured in a new ADR.
- [x] `.criv/` remains ignored and no scratch fixtures are tracked.

## Review

- [x] Code reviewed phase by phase.
- [x] `PLAN.md` updated if scope, APIs, or phase boundaries changed.
- [x] `TODO.md` checked only after the corresponding command or commit succeeds.
- [x] Every phase commit is clean and uses the exact drafted conventional
  commit message.
- [x] `plans/README.md` status for plan 017 is updated as work progresses.
- [x] All TODO items are checked off before plan 017 is marked DONE.

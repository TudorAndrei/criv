# TODO: Fix 2026-07-25 Audit Findings

## Phase 1: Characterize the two paths that rewrite artifacts

- [ ] Add a `check --fix` test group to `tests/cli_workflows.rs`, modeled on `check_json_output_is_valid_for_special_characters` (`tests/cli_workflows.rs:542`).
- [ ] Cover: a fixable Markdown file under `docs/` is rewritten on disk and reported.
- [ ] Cover: an already-clean Markdown file under `docs/` is left byte-identical.
- [ ] Add a `src/state.rs` unit test for `incremental_pattern_matches` (`src/state.rs:483-514`) with one ADR-local `policy.patterns` entry governing two source files.
- [ ] Assert the unchanged file's match is preserved byte-identically and the changed file's match is refreshed.
- [ ] Add a deletion case: a removed governed source file's match must not survive into the next state.
- [ ] Locally confirm a root-level `README.md` fix case fails with `refusing to write README.md outside allowed vault directory docs`; do not commit it yet.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `test(check): cover markdown fixes and incremental pattern reuse`

## Phase 2: Fix every Markdown file that check lints

- [ ] Read ADR-0044's Decision section before starting.
- [ ] Change `apply_markdown_fixes` (`src/check.rs:229-278`) to select the allowed write dir per destination: `docs_dir` inside the docs tree, `Path::new(".")` otherwise.
- [ ] Keep the `write_atomic_in` call unchanged; do not bypass or reimplement `prepare_confined_write`.
- [ ] Confirm no warning diagnostic and no second scope concept is introduced.
- [ ] Verify the `strip_prefix(write_scope.root)` guard at `src/check.rs:251-256` still rejects destinations outside the repository root.
- [ ] Commit the Phase 1 root-level `README.md` regression, asserting success and that the file is fixed on disk.
- [ ] Add a case asserting a file excluded in `.rumdl.toml` is neither linted nor rewritten.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `fix(check): fix every markdown file that check lints`

## Phase 3: Scan all sources when no path filter is given

- [ ] Add `PathScope<'a> { All, Globs(&'a [String]) }` to `src/structural.rs`.
- [ ] Change `find` (`src/structural.rs:60`) to take it and build the `GlobMatcher` only for `Globs`.
- [ ] Thread it through `find_pattern_id` (`src/structural.rs:225`) and `find_policy_pattern_entry` (`src/structural.rs:236`).
- [ ] `src/search.rs:127` (`Mode::Structural`) uses `All` when `paths` is empty, else `Globs`.
- [ ] `src/search.rs:152` (`search_pattern_id`) uses `All` when `paths` is empty, else `Globs`.
- [ ] `src/state.rs:564` uses `All` on the full-rebuild path.
- [ ] `src/state.rs:533`, `src/state.rs:548`, and `src/state.rs:569` use `Globs`, preserving empty-means-nothing.
- [ ] Confirm `search_rule` (`src/search.rs:155-165`) is unchanged.
- [ ] Confirm `find_policies_batch` is unchanged.
- [ ] Add a `src/structural.rs` test: `PathScope::All` scans all source files, `PathScope::Globs(&[])` scans none.
- [ ] Add a `src/state.rs` test: an incremental rebuild whose changed files are all out of scope produces zero matches.
- [ ] Add a `tests/cli_workflows.rs` regression for `criv search '<pattern>'` with no `--paths` and no `--lang`.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `fix(search): scan all sources when no path filter is given`

## Phase 4: Report file-relative note line numbers

- [ ] Change `split_frontmatter` (`src/vault.rs:501-521`) to return the number of lines consumed, both delimiters included.
- [ ] Apply the offset once at `src/vault.rs:484` (wiki links), `src/vault.rs:492` (headings), and `src/vault.rs:496` (C4 diagrams).
- [ ] Pass the offset into `c4::parse_diagrams` (`src/c4.rs:120`) and add it to the `start_line` given to `parse_mermaid_diagram`.
- [ ] Set the offset to zero in the frontmatter-parse-error branch (`src/vault.rs:461-478`), where `body` is the entire file.
- [ ] Confirm `src/c4_artifact.rs:205` is unchanged.
- [ ] Add a `src/vault.rs` test covering a note with and without frontmatter, asserting wiki-link and heading lines equal real file lines.
- [ ] Add a `src/check.rs` regression asserting a `broken-link` diagnostic line equals the real file line in a note with frontmatter.
- [ ] Review and update any test asserting body-relative numbers; confirm `src/c4.rs:494-497` still passes.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `fix(vault): report file-relative note line numbers`

## Phase 5: Reuse the cached source graph for single watch runs

- [ ] In `src/watch.rs:27-31`, pass `crate::source_graph::load_cached(root)` as `previous_graph` for the `--once` path.
- [ ] Apply the same treatment to the long-running startup rebuild.
- [ ] Add a `tests/cli_workflows.rs` test asserting a second consecutive `criv watch --once` reports zero changed files.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `perf(watch): reuse the cached source graph for single runs`

## Phase 6: Reclaim an abandoned watch lock

- [ ] Write the owning PID and start time into `.criv/watch.lock` in `WatchLock::acquire` (`src/watch.rs:210-227`).
- [ ] On `AlreadyExists`, reclaim the lock when the recorded process is not alive; otherwise fail as today.
- [ ] Treat an unreadable or malformed lock file as stale and reclaim it.
- [ ] Extend the error message at `src/watch.rs:216-219` to name the recovery step.
- [ ] Add a `src/watch.rs` test: a lock owned by a dead PID is reclaimed by `watch --once`.
- [ ] Add a `src/watch.rs` test: a lock owned by the current live process is still rejected.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `fix(watch): reclaim an abandoned watch lock`

## Phase 7: Resolve package-local lint tools

- [ ] Replace the one-variant `ToolCommand` (`src/enforce.rs:856-867`) with `Name(&'static str)` plus `Path(PathBuf)`.
- [ ] Change `tool_on_path` (`src/enforce.rs:852`) to probe `<root>/node_modules/.bin/`, `.obsidian/plugins/criv/node_modules/.bin/`, then `extensions/vscode-criv/node_modules/.bin/`, falling back to the bare name.
- [ ] Preserve the skip behavior and message at `src/enforce.rs:836-839` when nothing resolves.
- [ ] Confirm no ESLint fallback is introduced (ADR-0024).
- [ ] Add a `src/enforce.rs` test asserting a package-local binary is preferred over a bare name.
- [ ] Add a `src/enforce.rs` test asserting the skip message still appears when nothing resolves.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `fix(enforce): resolve package-local lint tools`

## Phase 8: Bring `criv init` under write confinement

- [ ] Read ADR-0044's Decision and Consequences before starting; this phase is required by it.
- [ ] Add `write_new_in` to `src/util.rs` on top of `prepare_confined_write` and `create_new_in`, preserving the "false if already exists" contract.
- [ ] Route `write_template` (`src/init.rs:347-358`) through it for all 16 templates.
- [ ] Route `write_hook` (`src/init.rs:247-262`) through it with allowed dir `.githooks`.
- [ ] Route the `.gitignore` append (`src/init.rs:79`) through a confined equivalent.
- [ ] Route both `.vscode/extensions.json` writes (`src/init.rs:109`, `src/init.rs:145`) off the unconfined `write_atomic`.
- [ ] Pre-canonicalize `root` in `init::run`, matching `src/init.rs:169-170`.
- [ ] Review the bare `fs::create_dir_all` calls at `src/init.rs:43`, `:44`, `:173` for symlinked ancestors.
- [ ] Reject rather than skip on a symlinked destination component, with an error naming the path.
- [ ] Add a Unix-only test for a symlinked template destination, modeled on `src/util.rs:573-590`.
- [ ] Add a Unix-only test for a symlinked `.githooks` directory; assert the outside target is not written.
- [ ] Confirm all 13 existing tests in `src/init/tests.rs` pass unchanged.
- [ ] `cargo test --workspace` passes.
- [ ] Commit: `fix(init): confine scaffolding and hook writes`

## Verification

- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo run --quiet -- check` passes.
- [ ] `cargo run --quiet -- enforce --stage ci` passes and reports oxlint as checked, not skipped.
- [ ] `mise run check` passes.
- [ ] New tests written for: `check --fix` scoping, incremental pattern reuse, empty-path structural search, frontmatter line offsets, warm `watch --once`, stale lock reclamation, and lint-tool resolution.
- [ ] Manual smoke test: `criv search 'fn $NAME() { $$$ }'` with no `--paths` returns matches (issue 1 reproduction now passes).
- [ ] Manual smoke test: `criv check --fix` in a repo with an unformatted root-level `README.md` succeeds and fixes the file (issue 3 reproduction now passes).
- [ ] Edge case tested: `criv check --fix` still refuses a destination outside the repository root and one reached through a symlink, at `allowed_dir = "."`.
- [ ] Behavior change acknowledged: `mise run fix` now reformats root-level Markdown it previously left alone.
- [ ] Manual smoke test: `criv watch` interrupted with Ctrl-C, then `git commit` succeeds without removing `.criv/watch.lock` by hand.
- [ ] Manual smoke test: `criv watch --once` twice in a row; the second run reuses the cached graph.
- [ ] Edge case tested: a note with no frontmatter still reports correct line numbers.
- [ ] Edge case tested: a note with malformed frontmatter reports line numbers against the whole file, with no double offset.
- [ ] Edge case tested: an incremental rebuild whose changed files fall entirely outside an ADR's `governs` scope produces zero matches, not a full rescan.
- [ ] Edge case tested: a non-decision note carrying `policy.patterns` with empty `governs` still matches nothing.
- [ ] Edge case tested: `criv search --rule ADR-NNNN` with no `--paths` is unchanged.
- [ ] No regression in `criv query diff`, `criv query coverage`, or `criv enforce --stage commit` output shape.
- [ ] `.criv/state.json` heading node IDs match real file lines after Phase 4 (`docs/adr/0001-local-cli-vault-architecture.md` H1 reports `#L14`, not `#L2`).
- [ ] `mise run perf` recorded before and after Phase 5; `watch_once_warm` improvement noted.

## Review

- [ ] Code reviewed.
- [ ] PLAN.md updated if approach changed during implementation.
- [ ] All phase commits are clean and describe their intent.
- [ ] TODO.md items all checked off.
- [ ] `ISSUES.md` issue statuses updated from Open to Fixed for issues 1, 2, 3, 4, 5, 6, 10, and 17.
- [ ] `criv init` in a scratch repo with a symlinked template destination errors and writes nothing outside the root.
- [ ] No file under a scope governed by ADR-0044 still uses `write_new`, unconfined `write_atomic`, or `append_line_if_missing`.
- [ ] Open Questions in PLAN.md resolved and the decisions recorded.
- [ ] Implementation matches ADR-0044 and ADR-0045; if it contradicts either, a superseding ADR was written rather than editing them (ADR-0012).
- [ ] `criv check` passes.
- [ ] `criv enforce --stage ci` passes and no longer reports oxlint as skipped.

# Plan 011: Add end-to-end coverage for the untested `criv query` subcommands

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/query.rs tests/cli_workflows.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition. (Plan 001 modifies
> `src/query.rs::load_snapshot` — if it landed, its snapshot-id validation is
> expected drift; read plans/001 before proceeding.)

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW (tests only — production code changes are out of scope)
- **Depends on**: plans/001 recommended first (it touches `load_snapshot`,
  which the diff tests exercise)
- **Category**: tests
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

`criv query` is a headline capability (README lines 18–21), but most of its
subcommands have zero end-to-end coverage: `callers`, `callees`,
`attack-surface`, `targets`, `cites`, `cited-by`, `orphan-docs`, `references`,
`governs`, `governing`, `next-adr-id`, and `diff` appear only in the dispatch
`match` — no test invokes them through the CLI. The graph internals below some
of them are unit-tested (`src/source_graph.rs` tests `attack_surface` and call
resolution), but argument parsing, row formatting, `--format json` output, the
required-arg error paths, and the snapshot/git-ref `diff` logic can all break
with the suite green. This plan adds table-driven CLI workflow tests over a
small fixture vault, plus a `diff` snapshot round-trip.

## Current state

Relevant files:

- `src/query.rs` — dispatch at lines 36–99 (excerpt below); `required_arg`
  error path at ~102; `diff` implementation at ~442–527 (`load_snapshot`,
  `load_git_state`, node/edge set comparison emitting
  `node_added`/`node_removed`/`edge_added`/`edge_removed` rows);
  `print_rows` handles `Format::Text` (one row per line) and `Format::Json`.
- `tests/cli_workflows.rs` — the single CLI integration test file
  (~1059 lines). Helpers at the top:

```rust
fn criv(root: &Path) -> Command {
    let mut command = Command::cargo_bin("criv").expect("criv binary");
    command.current_dir(root);
    command.env_remove("CI");
    command.env_remove("GITHUB_ACTIONS");
    command.env_remove("CRIV_BASE_REF");
    command.env_remove("GITHUB_BASE_REF");
    command
}

fn init(root: &Path) {
    criv(root)
        .args(["init", "--no-hooks", "--no-obsidian", "--no-skills"])
        .assert()
        .success();
}
```

- Existing coverage to NOT duplicate: `query coverage`, `query nodes`
  (+ `--format json` special-characters test at line 213), `query
  c4-elements`, `c4-relationships`, `c4-code` (lines 52–54, 128, 503–519).

Dispatch excerpt (`src/query.rs:38-99`), i.e. the full subcommand inventory
and each one's required positional arg:

| subcommand | positional | notes |
|---|---|---|
| `next-adr-id` | — | scans ADR ids, returns next |
| `callers` | symbol | `vault.source_graph().callers(symbol)` |
| `callees` | symbol | |
| `attack-surface` | — | |
| `targets` | note-id | |
| `cites` | note-id | |
| `cited-by` | note-id | |
| `orphan-docs` | — | |
| `references` | symbol | |
| `governs` | ADR-ID | |
| `governing` | symbol | |
| `coverage` | — (`--by` flag) | covered already |
| `nodes` | — (`--kind`, `--without-docs`) | covered already |
| `diff` | ref-a ref-b | two positionals; missing second → usage error "query `diff` requires <ref-a> <ref-b>" |
| anything else | | usage error "query `<other>` is not implemented in this MVP" |

Fixture-vault building blocks (all visible in the existing workflow test at
`tests/cli_workflows.rs:26-60`):

- `init(root)` writes `criv.toml`, `docs/`, `docs/adr/`.
- Writing `src/lib.rs` with a `pub fn run()` then `criv watch --once`
  produces state where node id `src/lib.rs#fn:run` exists (asserted at
  line ~57).
- ADR notes live in `docs/adr/NNNN-title.md` with YAML frontmatter (id,
  kind, title, status, and optionally `governs:`); look at existing enforce
  tests further down `tests/cli_workflows.rs` for a frontmatter example to
  copy exactly, and at `docs/adr/0001-*.md` in this repo for the real shape.
- Notes citing each other use `[[wiki-links]]`; a note referencing source
  uses source selectors (see `docs/` in this repo, and the `source-wikilink`
  guidance in `src/check.rs:981`).

`diff` semantics (`src/query.rs:442-527`): each ref is resolved by
`load_snapshot` — `latest` reads `.criv/latest`; a hex hash reads
`.criv/snapshots/<hash>.json`; anything else falls back to
`git show <ref>:.criv/state.json`. Snapshots are written by `watch --once`
(`State::write_snapshot`), and the snapshot hash is the content of
`.criv/latest`.

Conventions: `assert_cmd` + `predicates`, one `#[test]` fn per scenario,
`TempDir` per test. Conventional commits.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Just these tests | `cargo test --test cli_workflows` | exit 0 |
| Full suite | `cargo test --workspace` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `tests/cli_workflows.rs`

**Out of scope** (do NOT touch):
- `src/query.rs` or any production code. If a test exposes a real bug, STOP
  and report it (that's a success condition of this plan, not a license to
  fix it here).
- Existing tests in `cli_workflows.rs` — add, don't refactor.

## Git workflow

- Single conventional commit: `test(query): cover query subcommands end to end`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Build one shared fixture helper

Add a helper `fn query_fixture(root: &Path)` (name it in the file's style)
that assembles a vault exercising every relationship the queries need:

- two source files where one function calls the other (so `callers`/`callees`
  /`references` return non-empty rows), e.g. `src/lib.rs` with `pub fn run()`
  calling a local `fn helper()`;
- one ADR (`docs/adr/0001-test-decision.md`) with `status: accepted` and
  `governs: ["src/**/*.rs"]` (copy frontmatter shape from an existing enforce
  test in this file);
- one doc note (`docs/guide.md`) that cites the ADR via wiki-link and
  targets a source file (so `targets`, `cites`, `cited-by` have data);
- one orphan note (`docs/orphan.md`) nothing links to (for `orphan-docs`);
- run `criv watch --once` at the end so `.criv/state.json`, `.criv/latest`,
  and a snapshot exist.

Iterate: run each query manually against your fixture
(`cargo run --quiet -- query <name> <arg>` in a scratch copy) to learn the
exact row text BEFORE writing assertions — assert on stable substrings
(paths, ids), not whole lines, unless the line is obviously stable.

**Verify**: the fixture helper compiles and a smoke test running
`query next-adr-id` on it succeeds and prints `ADR-0002` (next after 0001 —
confirm the actual output format first).

### Step 2: Table the read-only queries

Add tests (grouped sensibly, e.g. one test fn per query family):

- `query next-adr-id` → contains the expected next id.
- `query callers <callee-symbol>` → contains the caller's symbol/path;
  `query callees <caller-symbol>` → contains the callee.
- `query attack-surface` → succeeds; assert on a stable expectation from your
  fixture (check what qualifies as attack surface in
  `src/source_graph.rs`'s `attack_surface` tests first — public/exported
  functions).
- `query targets <note-id>`, `query cites <note-id>`,
  `query cited-by <ADR-id>`, `query governs ADR-0001`,
  `query governing <symbol-or-path>`, `query references <symbol>`,
  `query orphan-docs` (→ contains `orphan.md`, does not contain `guide.md`).
- `--format json` variant for at least `callers` and `governs`: stdout parses
  as JSON (`serde_json::from_str::<serde_json::Value>` — the crate is
  available since `serde_json` is a dependency of the binary; if it is not
  available to the test target, assert on `predicate::str::starts_with("[")`
  and balanced-bracket substrings instead — check how the existing JSON test
  at line 213 asserts and copy that).
- Error paths: `query callers` with NO positional → failure, stderr contains
  the `required_arg` message (run it once to capture the wording);
  `query bogus` → failure, stderr contains "is not implemented".

**Verify**: `cargo test --test cli_workflows` → exit 0, new tests listed.

### Step 3: Cover `diff` round-trip

New test:

1. Build the fixture; `watch --once`; read `.criv/latest` → `hash_a`.
2. `query diff latest latest` → success; output contains no
   `node_added`/`node_removed` rows (identical refs).
3. Add a new function to `src/lib.rs`; `watch --once` again; read new
   `.criv/latest` → `hash_b` (assert `hash_a != hash_b`).
4. `query diff <hash_a> <hash_b>` → success; stdout contains a `node_added`
   row mentioning the new function; `query diff <hash_b> <hash_a>` →
   contains the corresponding `node_removed`.
5. Error path: `query diff <hash_a>` (one arg) → failure, stderr contains
   "requires <ref-a> <ref-b>". Nonexistent ref:
   `query diff nonexistent latest` → failure, stderr contains
   "does not resolve".

**Verify**: `cargo test --test cli_workflows` → exit 0.

### Step 4: Full gate

**Verify**:
- `cargo test --workspace` → exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `git status` shows only `tests/cli_workflows.rs` modified

**Commit**: `test(query): cover query subcommands end to end`

## Test plan

This plan IS the test plan; the deliverable is the tests in Steps 2–3.
Success bar: every dispatch arm in `src/query.rs:38-92` is exercised by at
least one CLI invocation (assert this yourself:
`grep -o '"[a-z-]*" =>' src/query.rs` and cross-check your test list).

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test --test cli_workflows` exits 0
- [ ] Every query subcommand token (`next-adr-id`, `callers`, `callees`,
      `attack-surface`, `targets`, `cites`, `cited-by`, `orphan-docs`,
      `references`, `governs`, `governing`, `diff`) appears in
      `tests/cli_workflows.rs` inside a `criv(root).args([...])` invocation
      (`grep` each token)
- [ ] Both `diff` error paths and the two usage-error paths are asserted
- [ ] At least two queries asserted under `--format json`
- [ ] `cargo test --workspace` and clippy exit 0
- [ ] No files besides `tests/cli_workflows.rs` modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- Any query returns output that looks WRONG for the fixture (e.g. `callers`
  returns the callee, `orphan-docs` lists a cited note) — that's a product
  bug; report it with the fixture and output rather than writing an assertion
  that enshrines the bug.
- A query subcommand cannot produce non-empty output from any reasonable
  fixture (report — it may be dead or needs data the fixture can't express).
- The frontmatter/link syntax you copied doesn't validate (`criv check`
  fails on the fixture vault) after two attempts — report the diagnostics.

## Maintenance notes

- These tests pin the CLI's row text loosely (substrings). If output
  formatting changes deliberately, update assertions in the same PR — the
  point is catching *accidental* changes.
- Plan 015 (SARIF/annotations spike) may add output formats; these tests
  define the current text/JSON baseline it must not break.
- Follow-up deferred: property-style tests for `diff` (arbitrary state pairs)
  and unit tests for `json_string_set`/`json_edge_set` in `src/query.rs`.

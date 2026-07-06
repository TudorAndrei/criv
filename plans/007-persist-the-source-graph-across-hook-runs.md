# Plan 007: Persist the fingerprinted source graph so hook runs stop cold-parsing the whole tree

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/source_graph.rs src/vault.rs src/watch.rs src/state.rs src/util.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition. Plans 005 and 006 intentionally
> touch adjacent code — diffs limited to their scopes are expected; read
> their plan files if the drift is theirs.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: MED
- **Depends on**: plans/006 (recommended first — it shrinks per-run work and
  avoids rebasing this plan's measurements)
- **Category**: perf
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

Every git commit runs `criv watch --once`, `criv check`, and
`criv enforce --stage commit` as separate processes (generated hooks +
`hk.pkl`). Each process calls `Vault::load`, which tree-sitter-parses **every
source file from scratch**: the incremental-reuse machinery
(`SourceGraph::build_incremental` with fingerprint comparison) exists and
works, but one-shot commands always pass `previous_graph = None`, so it only
helps the long-running `criv watch` loop. Persisting the fingerprinted graph
to `.criv/` lets every subsequent invocation re-parse only changed files.
This attacks the dominant fixed cost of the hook pipeline (three cold full
parses per commit).

Important scoping fact discovered during planning: `criv enforce` does NOT
fully duplicate `criv check` — `check::run` additionally does markdown
formatting, C4-interface drift, and vault-wide policy scans, while `enforce`
runs `check::validate` plus changed-file-scoped policies and native linters.
So merging the hook steps would change behavior; the safe consolidation is
making each process cheap, which is what this plan does.

## Current state

Relevant files:

- `src/source_graph.rs` — `SourceGraph` struct (line 10) and
  `build_incremental` (line 210). All graph types derive
  `Debug, Default, Clone` but NOT serde traits.
- `src/vault.rs` — `Vault::load` (line 127) calls
  `Self::load_incremental(root, None)`; `load_incremental` (line 131) calls
  `SourceGraph::build_incremental(root, &source_files, previous_graph)` at
  line ~183.
- `src/watch.rs` — long-running mode threads the previous graph through
  rebuilds; `watch --once` takes the `rebuild(root, None)` path (line 26).
- `src/state.rs` — the existing `.criv` write conventions: `write_atomic`
  helper (from `src/util.rs`), a `schema` version field on state
  (line 17/366), `State::write` to `.criv/state.json` (line 384–392).
- `src/util.rs` — `write_atomic`.

Excerpt — the struct to persist (`src/source_graph.rs:10-16`):

```rust
#[derive(Debug, Default, Clone)]
pub(crate) struct SourceGraph {
    pub(crate) files: BTreeMap<String, SourceFile>,
    file_fingerprints: BTreeMap<String, String>,
    changed_files: Vec<String>,
    symbol_index: BTreeMap<String, Vec<SymbolId>>,
}
```

Excerpt — the reuse logic that is dead for one-shot runs
(`src/source_graph.rs:210-230`):

```rust
    pub(crate) fn build_incremental(
        root: &Path,
        source_files: &[String],
        previous: Option<&Self>,
    ) -> Result<Self> {
        let mut graph = Self::default();
        for source_file in source_files {
            let fingerprint = source_file_fingerprint(root, source_file)?;
            let reused = previous
                .filter(|previous| {
                    previous.file_fingerprints.get(source_file) == Some(&fingerprint)
                })
                .and_then(|previous| previous.files.get(source_file).cloned());
            let parsed = if let Some(parsed) = reused {
                parsed
            } else {
                graph.changed_files.push(source_file.clone());
                let contents = read_source_to_string(root, source_file)?;
                parse_source_file(source_file, &contents)
            };
```

Excerpt — the entry point (`src/vault.rs:127-131`):

```rust
    pub(crate) fn load(root: &Path) -> Result<Self> {
        Self::load_incremental(root, None)
    }
```

Repo conventions to honor:

- `.criv/` is local generated state, gitignored, already consumed by editor
  plugins. New cache files belong there.
- All `.criv` writes go through `write_atomic` (see `State::write`,
  `src/state.rs:384-392`).
- Generated state carries an explicit schema string
  (`src/state.rs:17`, `schema: STATE_SCHEMA` at 366) and consumers validate
  it. The graph cache must do the same so a criv upgrade never deserializes a
  stale shape.
- `serde` with derive is already a dependency (`Cargo.toml`).
- ADR-0007 (docs/adr/0007-content-addressed-state-and-diffing.md) governs
  `.criv` state/snapshot design — read it before changing anything under
  `.criv/`; the graph cache is a private cache, not part of the
  content-addressed snapshot model, and must not be written into
  `.criv/snapshots/`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `cargo test --workspace` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Self-check | `cargo run --quiet -- check` | exit 0 |
| State refresh | `cargo run --quiet -- watch --once` | exit 0; writes `.criv/` |
| Perf measure | `mise run perf` | completes; record before/after |
| Hook pipeline timing | `time (cargo run --quiet -- watch --once && cargo run --quiet -- check && cargo run --quiet -- enforce --stage ci)` | faster on 2nd run |

## Scope

**In scope** (the only files you should modify):
- `src/source_graph.rs` (serde derives + cache load/save functions)
- `src/vault.rs` (`Vault::load` uses the persisted graph)
- `src/watch.rs` (persist after rebuilds; `--once` benefits automatically via
  `Vault::load`)
- `src/state.rs` only if a shared schema-version constant naturally lives
  there; otherwise don't touch it
- `tests/cli_workflows.rs` (new end-to-end cache test)

**Out of scope** (do NOT touch):
- `hk.pkl`, `.githooks` templates, `src/init/templates.rs` — the hook contract
  (three separate commands) stays as-is in this plan.
- `check::run` / `enforce::run` internal structure — no merging of their
  responsibilities.
- `.criv/state.json` / snapshot formats — editor plugins parse them; the graph
  cache is a NEW file, additive only.
- The `SourceIndex`/fff layer (plan 005 owns that).

## Git workflow

- Conventional commits, e.g.
  `perf(graph): persist fingerprinted source graph in .criv` — split into
  serde-derive commit + wiring commit if that keeps each green.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Measure the baseline

Run the hook-pipeline timing command (table above) twice on this repo and
record the second run's wall time. This is the number Step 5 must beat.

**Verify**: you have a recorded baseline time.

### Step 2: Make `SourceGraph` serializable

Add `serde::{Serialize, Deserialize}` derives to `SourceGraph` and every type
it contains transitively: `SourceFile`, `Symbol`, `SymbolId`, `SymbolRange`,
`Call`, `SymbolKind`, and any enum/struct the compiler then demands (follow
the errors). Private fields (`file_fingerprints`, `changed_files`,
`symbol_index`) serialize too — the whole struct round-trips.
`changed_files` is per-build scratch: annotate it `#[serde(skip)]` so a loaded
cache starts with an empty changed list (verify nothing reads `changed_files`
from a *previous* graph — grep `changed_files` usages; today it is only read
from the freshly built graph in `src/state.rs`/`src/watch.rs`).

**Verify**: `cargo test --workspace` → exit 0 (derives only, no behavior).

### Step 3: Add cache load/save

In `src/source_graph.rs`, add:

```rust
const GRAPH_CACHE_SCHEMA: &str = "criv.source-graph/1";

#[derive(Serialize, Deserialize)]
struct GraphCacheFile {
    schema: String,
    graph: SourceGraph,
}

pub(crate) fn load_cached(root: &Path) -> Option<SourceGraph> { ... }
pub(crate) fn store_cached(root: &Path, graph: &SourceGraph) -> Result<()> { ... }
```

- Cache path: `.criv/source-graph.json`.
- `load_cached` returns `None` on: missing file, unreadable file, JSON parse
  error, or schema mismatch. Never propagate an error — a bad cache means
  "full parse", not a failed command.
- `store_cached` serializes with `serde_json` and writes via
  `crate::util::write_atomic`, creating `.criv/` if needed (match how
  `State::write` does it, `src/state.rs:384-392`).

**Verify**: `cargo test --workspace` → exit 0, including a new unit test that
round-trips a small graph through store/load in a temp dir (model on the
temp-dir tests in `src/state.rs`, e.g. around line 917).

### Step 4: Wire it into `Vault::load` and the watch loop

- `Vault::load` (`src/vault.rs:127`): load the cache and pass it through:

```rust
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let cached = crate::source_graph::load_cached(root);
        Self::load_incremental(root, cached.as_ref())
    }
```

- After a successful build inside `load_incremental` (or in the callers —
  pick ONE place; inside `load_incremental` right after
  `SourceGraph::build_incremental` succeeds is simplest), call
  `store_cached(root, &source_graph)`. Only store when `config.source_index`
  is true (the else-branch uses `SourceGraph::default()` — don't cache that).
  A store failure should be a non-fatal warning path — decide: propagate the
  error only if `.criv` exists and is writable failures are unexpected;
  simplest correct behavior is to propagate (matching how state writes
  behave today), which is acceptable.
- `src/watch.rs`: the long-running loop already threads `previous_graph`
  in-memory; it now ALSO benefits from persistence on restart with zero
  changes, because `rebuild` → `Vault::load*`. Check `rebuild`'s actual call
  (`src/watch.rs:25-31` and the loop body) — if it calls
  `Vault::load_incremental(root, Some(&source_graph))` directly, leave that
  path alone; the store in `load_incremental` covers it.
- **Correctness invariant**: `build_incremental` validates every file's
  fingerprint before reuse, so a stale or corrupt-but-parseable cache can only
  cause re-parsing or correct reuse — never wrong symbols. The schema guard
  covers shape changes. State this in a code comment on `GRAPH_CACHE_SCHEMA`:
  bump the schema string whenever `parse_source_file` output or any serialized
  type changes meaning.

**Verify**:
- `cargo test --workspace` → exit 0
- `rm -rf .criv && cargo run --quiet -- watch --once && cargo run --quiet -- check`
  → exit 0, `.criv/source-graph.json` exists
- `cargo run --quiet -- check` twice; second run must produce identical output
  to the first (save + diff)
- Corruption drill: `echo garbage > .criv/source-graph.json && cargo run --quiet -- check`
  → exit 0 (silent full parse), cache rewritten valid

### Step 5: Measure and gate

Repeat the Step 1 timing. The second pipeline run should be measurably faster
(the win scales with repo size; on this repo expect a clear improvement in the
check/enforce steps — record exact numbers).

**Verify**:
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `mise run perf` → record before/after in the commit message body
- `cargo run --quiet -- check` → exit 0 (the repo's own vault still validates)

**Commit**: `perf(graph): persist fingerprinted source graph in .criv`

## Test plan

- Unit (in `src/source_graph.rs` `mod tests`):
  - store/load round-trip equals the original graph (files, fingerprints,
    symbol index; `changed_files` empty after load).
  - `load_cached` returns `None` for garbage JSON and for a wrong schema
    string.
- End-to-end (in `tests/cli_workflows.rs`, model on
  `init_check_watch_query_search_and_enforce_workflow` at line 26):
  - run `watch --once`, assert `.criv/source-graph.json` exists;
  - modify one source file, run `check`, assert it still validates and the
    cache file's fingerprint entry for that file changed (parse the JSON).
- Verification: `cargo test --workspace` → all pass.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test --workspace` exits 0 including the new unit + CLI tests
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `.criv/source-graph.json` is created by `watch --once` and reused by
      `check` (verify by timing or by adding a temporary debug print you then
      remove — do not ship debug output)
- [ ] Corrupt-cache drill passes (garbage file → command still exits 0)
- [ ] `cargo run --quiet -- check` output unchanged before vs after (diff)
- [ ] Recorded before/after pipeline timings show improvement
- [ ] `git status` clean outside in-scope files (`.criv/` is gitignored —
      confirm nothing under `.criv/` became tracked)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- Any serialized type contains non-serde-able data (raw tree-sitter nodes,
  lifetimes) — inspect `SourceFile`/`Symbol` first; if parse trees are stored
  (not just extracted symbols), this plan's approach is wrong and needs a
  redesign. (Planning-time reading says only extracted data is stored, but
  verify.)
- `source_file_fingerprint` (src/source_graph.rs:382) turns out to be
  mtime/size-based rather than content-based AND tests show reuse of stale
  content — report; do not silently switch fingerprint algorithms.
- ADR-0007 says something that contradicts a private `.criv/source-graph.json`
  cache file (read it in Step 3) — reconcile with the operator first.
- The measured improvement in Step 5 is negligible on this repo — report the
  numbers before committing; the operator may still want it for larger vaults,
  but that's their call.
- Editor plugins (`.obsidian/plugins/criv`, `extensions/vscode-criv`) turn out
  to enumerate or validate ALL files under `.criv/` (they should read only
  `state.json` — grep their `src/` for `.criv` to confirm).

## Maintenance notes

- **Schema discipline**: any change to `parse_source_file` output, symbol
  selectors, or serialized types MUST bump `GRAPH_CACHE_SCHEMA`, or stale
  caches will replay old parses. Reviewers of future `source_graph.rs` changes
  should check for this; consider a test asserting the schema string changes
  when the serialized shape does (hard to automate — at minimum the comment).
- Follow-up explicitly deferred: merging check/enforce into one process or
  hk step (hook-contract change, needs an ADR); parallelizing parses.
- Reviewer: scrutinize the store timing (after successful build only) and
  that `--no-source-index` configs (`config.source_index == false`) never
  write the cache.

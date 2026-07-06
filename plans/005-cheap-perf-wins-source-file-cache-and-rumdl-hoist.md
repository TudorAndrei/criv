# Plan 005: Cache the source-file list per index instance and hoist rumdl rule construction

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/source_index.rs src/check.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: perf
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

`criv check` / `criv enforce` / `criv watch --once` run on every commit via git
hooks, so per-run latency is user-facing. Two hot-loop wastes:

1. **Per-link source-file list rebuild.** Every wiki-link that might be a
   source reference goes through `FffSourceIndex::resolve_partial_path`, which
   rebuilds the complete deduplicated source-file list (locking each fff
   picker, iterating all files, canonicalizing) and often runs a fuzzy search
   too. Validating L links over F source files is O(L×F) with lock traffic,
   when the file list is immutable for the lifetime of a one-shot command.
2. **Per-file rumdl rule set construction.** Markdown validation rebuilds the
   entire rumdl rule set (dozens of boxed `dyn Rule` objects) for every
   Markdown file instead of once per run.

Both fixes are local memoizations with no behavior change.

## Current state

Relevant files:

- `src/source_index.rs` — `FffSourceIndex` struct (~line 56) and its methods.
  `source_files()` (~line 147) builds the list; `resolve_partial_path()`
  (~lines 369–397) calls it per resolution; `entries()` (~line 406) also calls
  it once per state build.
- `src/check.rs` — markdown loop at lines ~114–121 calls `rules_for_file`
  per file; `rules_for_file` at lines ~226–233; `apply_markdown_fixes`
  (~line 195) also calls `rules_for_file` (only in `--fix` mode).
- `src/vault.rs` — `resolve_source_path` (line ~293) delegates to
  `resolve_partial_path`; callers include link validation in `src/check.rs`
  (`canonical_source_target` at check.rs:974, 1033) and `src/query.rs:208,263`.

Excerpt — the struct (`src/source_index.rs:56-62`):

```rust
pub(crate) struct FffSourceIndex {
    root: PathBuf,
    source_roots: Vec<String>,
    source_excludes: GlobMatcher,
    pickers: Vec<ScopedPicker>,
    explicit_files: Vec<String>,
}
```

Excerpt — the per-resolution rebuild (`src/source_index.rs:369-383`):

```rust
    fn resolve_partial_path(&self, path: &str) -> Option<(String, bool)> {
        if path.is_empty() || path.starts_with("match:") {
            return None;
        }

        let path = path.trim();
        let source_files = self.source_files().ok()?;
        if source_files.iter().any(|source_file| source_file == path) {
            return Some((path.to_string(), false));
        }

        let fff_matches = self
            .fuzzy_files(path, 50)
            ...
```

Excerpt — the rumdl loop (`src/check.rs:114-121` and `226-233`):

```rust
    for rel_path in files {
        let path = root.join(&rel_path);
        let mut contents = crate::util::read_to_string(&path)?;
        if fix {
            apply_markdown_fixes(&path, &rel_path, &mut contents, &config, &mut diagnostics)?;
        }

        let rules = rules_for_file(&path, &config);
```

```rust
fn rules_for_file(path: &Path, config: &RumdlConfig) -> Vec<Box<dyn Rule>> {
    let rules = filter_rules(&all_rules(config), &config.global);
    let ignored_rules = config.get_ignored_rules_for_file(path);
    if ignored_rules.is_empty() {
        return rules;
    ...
```

**Safety facts verified during planning** (the executor should re-confirm the
first one — see STOP conditions):

- The long-running `criv watch` keeps one `FffSourceIndex` alive across file
  changes, but that instance is only used for `source_fingerprint()`
  (`src/watch.rs:39,71-74`), which iterates the pickers directly and does NOT
  call `source_files()`. Every other `FffSourceIndex` is constructed fresh per
  command inside `Vault::load` (`src/vault.rs:172-185`). So caching
  `source_files()` per instance cannot stale the watch fingerprint.
- `rumdl_lib::rules::{all_rules, filter_rules}` are pure functions of the
  config; `get_ignored_rules_for_file` is the only per-file part.

Conventions: unit tests in `#[cfg(test)] mod tests` at the bottom of each file
(`src/source_index.rs:522`, `src/check.rs:1202`); conventional commits (recent
example: `perf(search): limit unfiltered file search candidates`).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `cargo test --workspace` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Self-check | `cargo run --quiet -- check` | exit 0 |
| Perf smoke (optional) | `mise run perf` | completes; note before/after timings |

## Scope

**In scope** (the only files you should modify):
- `src/source_index.rs`
- `src/check.rs`

**Out of scope** (do NOT touch):
- `src/search.rs` `fuzzy_files` limits — tuned recently
  (`perf(search): limit unfiltered file search candidates`); leave alone.
- `src/vault.rs`, `src/watch.rs` — no signature or lifecycle changes.
- Any change to which files are indexed or how paths are canonicalized
  (`indexed_path`, `prefixed_path`).
- The larger check/enforce pipeline consolidation — that is plan 007.

## Git workflow

- Conventional commits, one per step:
  `perf(index): cache the source file list per index instance` and
  `perf(check): build the rumdl rule set once per run`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Cache `source_files()` in `FffSourceIndex`

Add a `std::sync::OnceLock<Vec<String>>` field to the struct:

```rust
use std::sync::OnceLock;

pub(crate) struct FffSourceIndex {
    root: PathBuf,
    source_roots: Vec<String>,
    source_excludes: GlobMatcher,
    pickers: Vec<ScopedPicker>,
    explicit_files: Vec<String>,
    source_files_cache: OnceLock<Vec<String>>,
}
```

Initialize `source_files_cache: OnceLock::new()` in `FffSourceIndex::new`.
Rename the existing `source_files` body to a private `collect_source_files_now`
(or similar) and make `source_files` return the cached list:

```rust
    fn source_files(&self) -> Result<Vec<String>> {
        if let Some(cached) = self.source_files_cache.get() {
            return Ok(cached.clone());
        }
        let files = self.collect_source_files_now()?;
        Ok(self
            .source_files_cache
            .get_or_init(|| files)
            .clone())
    }
```

(The clone keeps the existing return type; callers iterate it once. If you
prefer to avoid clones, changing the trait method to return `&[String]` is NOT
in scope — it ripples through the `SourceIndex` trait and `EmptySourceIndex`.)

Keep the error path honest: if collection fails, return the error and leave
the cache unset so a retry can succeed.

**Verify**: `cargo test --workspace` → exit 0 (the `mod tests` at
`src/source_index.rs:522` exercises resolution paths).

**Commit**: `perf(index): cache the source file list per index instance`

### Step 2: Hoist rumdl base-rule construction out of the per-file loop

In `src/check.rs`, split `rules_for_file` so the base set is computed once.
Shape:

```rust
fn base_rules(config: &RumdlConfig) -> Vec<Box<dyn Rule>> {
    filter_rules(&all_rules(config), &config.global)
}

fn rules_for_file<'a>(
    path: &Path,
    config: &RumdlConfig,
    base: &[Box<dyn Rule>],
) -> Vec<Box<dyn Rule>> { ... }
```

Note: `Box<dyn Rule>` may not be `Clone`. Check how the current code filters
(`rules_for_file` currently *consumes* a fresh `all_rules` vec). If cloning
boxed rules isn't possible, the fallback that still wins: hoist only for the
common case — compute `config.get_ignored_rules_for_file(path)` first, and
when it's empty (typical), reuse a lazily-built shared base set via
`all_rules(config)` called once per run and passed by reference into
`rumdl_lib::lint` if its signature accepts `&[Box<dyn Rule>]` (check the
existing call at `src/check.rs:122-126` — it already passes `&rules`). If the
signature fight isn't winnable in ~30 minutes, STOP and report; don't force an
awkward refactor for a small win.

Also update `apply_markdown_fixes` (`src/check.rs:195-202`) to accept the same
prebuilt base if the refactor makes that natural; `--fix` mode is not hot, so
leaving it calling the old path is acceptable.

**Verify**: `cargo test --workspace` → exit 0; `cargo run --quiet -- check` →
exit 0 with identical diagnostics to before the change (run it on this repo
before and after; compare output).

**Commit**: `perf(check): build the rumdl rule set once per run`

### Step 3: Full gate and optional measurement

**Verify**:
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test --workspace` → exit 0
- `cargo run --quiet -- check` → exit 0
- Optional: `mise run perf` before/after; record the delta in the commit
  message body if you measured it.

## Test plan

- `src/source_index.rs` `mod tests`: add a test asserting `source_files()`
  returns identical results on two consecutive calls on the same instance
  (guards the cache) — model on the existing tests in that module.
- Existing link-resolution and check tests cover behavior preservation.
- Verification: `cargo test --workspace` → all pass.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test --workspace` exits 0
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `grep -n 'OnceLock' src/source_index.rs` → match in the struct + init
- [ ] `grep -n 'all_rules' src/check.rs` shows `all_rules` is no longer called
      inside the `for rel_path in files` loop (check mode; `--fix` path exempt)
- [ ] `cargo run --quiet -- check` output on this repo is identical before vs
      after (save both to files and `diff` them)
- [ ] `git status` clean outside in-scope files
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- You find ANY call path where a single `FffSourceIndex` instance is expected
  to observe filesystem changes between `source_files()` calls. Verify by
  grepping `source_files()` callers and re-reading `src/watch.rs:29-80`: the
  long-lived watch index must only use `source_fingerprint()`. If that has
  changed, the cache is unsafe as designed — report.
- `rumdl_lib`'s `lint` signature or rule types make the hoist require changes
  to more than `src/check.rs` internals.
- Any check diagnostic differs between before/after runs on this repo.

## Maintenance notes

- If a future feature mutates the index in-place during a command (e.g.
  incremental re-index without rebuilding `FffSourceIndex`), the `OnceLock`
  cache must be dropped or invalidated — grep for `source_files_cache` then.
- Plan 007 (hook pipeline consolidation) builds on the same hot path; land
  this first, it shrinks what 007 has to measure.
- Reviewer: watch for accidental behavior change in ambiguous-path resolution —
  the cache must not change ordering (the list is a sorted `BTreeSet` collect).

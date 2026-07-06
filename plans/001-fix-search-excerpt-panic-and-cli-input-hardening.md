# Plan 001: Fix the `search --notes` excerpt panic and harden two small CLI input paths

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/search.rs src/enforce.rs src/query.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug + security
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

`criv search --notes <query>` computes a match offset in a lowercased copy of a
note body and then slices the **original** body with that byte offset. Unicode
lowercasing is not byte-length-preserving (e.g. `İ` U+0130 is 2 bytes but its
lowercase form is 3 bytes), so any note containing such characters before an
ASCII match makes the offset land inside a multi-byte character or past the end
of the original string. That panics with "byte index N is not a char boundary" —
and release builds use `panic = "abort"`, so the whole command dies on ordinary
repo Markdown. Even when it doesn't panic, the excerpt is silently taken from
the wrong place. Two adjacent small hardening gaps ride along in this plan
because they're each a few lines in files this plan already verifies:
repo-derived file paths are passed to external linters without an end-of-options
`--` separator, and `criv query diff <id>` joins a raw CLI argument into a
filesystem path.

## Current state

Relevant files:

- `src/search.rs` — note search. `excerpt()` at lines 442–459 has the bug. It
  is called from `note_score()` (line ~424), which is called per note from
  `lexical_notes()` (lines 240–247), reached via `criv search --notes <text>`.
- `src/enforce.rs` — `run_optional_tool()` at lines 566–581 builds the oxlint /
  ruff subprocess.
- `src/query.rs` — `load_snapshot()` at lines 472–489 builds the snapshot path.

Excerpt of the buggy function (`src/search.rs:442-459`):

```rust
fn excerpt(body: &str, query_terms: &[String]) -> String {
    let body_lower = body.to_lowercase();
    let Some(offset) = query_terms
        .iter()
        .filter_map(|term| body_lower.find(term))
        .min()
    else {
        return String::new();
    };
    let start = body[..offset]
        .rfind(|ch: char| ['.', '\n'].contains(&ch))
        .map(|index| index + 1)
        .unwrap_or(0);
    let end = body[offset..]
        .find(|ch: char| ['.', '\n'].contains(&ch))
        .map(|index| offset + index)
        .unwrap_or_else(|| body.len());
    body[start..end].trim().to_string()
}
```

The bug: `offset` indexes `body_lower`, but every subsequent slice indexes
`body`. `query_terms` are already lowercased (see `tokenize()` just above
`excerpt` in the same file — it does `to_ascii_lowercase()` per token).

Excerpt of the linter invocation (`src/enforce.rs:566-581`):

```rust
fn run_optional_tool(
    root: &Path,
    label: &str,
    command: ToolCommand,
    files: &[String],
) -> Result<usize> {
    if files.is_empty() {
        return Ok(0);
    }

    let mut process = Command::new(command.program());
    process.current_dir(root);
    if label == "Ruff" {
        process.arg("check");
    }
    process.args(files);
```

`files` are vault-relative source paths. A committed file whose name starts
with `-` would be parsed by oxlint/ruff as a flag, not a path.

Excerpt of the snapshot loader (`src/query.rs:472-483`):

```rust
fn load_snapshot(root: &Path, id: &str) -> Result<serde_json::Value> {
    let hash = if id == "latest" {
        fs::read_to_string(root.join(".criv/latest"))?
            .trim()
            .to_string()
    } else {
        id.to_string()
    };
    let path = root.join(".criv/snapshots").join(format!("{hash}.json"));
```

`id` comes straight from the CLI (`criv query diff <left> <right>`). An id
containing `/` or `..` escapes `.criv/snapshots/`. When the joined path does
not exist, the function falls back to `load_git_state(root, id)` which passes
the id to `git show` as a plain argument — that fallback is fine and must keep
working, because git refs (e.g. `HEAD~1`, `main`) are valid ids. Snapshot
files are named by blake3 hex hashes (lowercase hex); see `.criv/snapshots/`
after running `criv watch --once`.

Repo conventions:

- Unit tests live in a `#[cfg(test)] mod tests` block at the bottom of each
  source file: `src/search.rs:509`, `src/enforce.rs:656`, `src/query.rs:559`.
  Match the existing test style in those modules.
- Commit messages are conventional commits, e.g. `fix(search): report invalid
  regex grep queries` (from `git log`).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `cargo test --workspace` | exit 0, all pass |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Format | `cargo fmt --all` | exit 0 |
| Self-check | `cargo run --quiet -- check` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `src/search.rs`
- `src/enforce.rs`
- `src/query.rs`

**Out of scope** (do NOT touch, even though they look related):
- `src/source_index.rs` and `src/source_paths.rs` — the path-confinement
  helpers there are correct; do not refactor them into `query.rs`.
- The `semantic_notes` / `embeddings` code path in `src/search.rs` (behind
  `#[cfg(feature = "embeddings")]`) — unrelated feature-gated code.
- `load_git_state()` in `src/query.rs` — the git fallback is correct as-is.

## Git workflow

- Branch: work on a branch if the operator's workflow requires one; otherwise
  commit to the current branch.
- One commit per step below; conventional-commit style.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Fix `excerpt()` to search and slice the same buffer

In `src/search.rs`, change `excerpt()` so the offset and all slices refer to
the same string. The simplest correct shape: find the offset in `body_lower`,
then do `start`/`end` scanning and the final slice on `body_lower` as well —
but that changes the excerpt's casing. To preserve the original casing in
output, map the lowered offset back safely instead:

```rust
fn excerpt(body: &str, query_terms: &[String]) -> String {
    let body_lower = body.to_lowercase();
    let Some(lower_offset) = query_terms
        .iter()
        .filter_map(|term| body_lower.find(term))
        .min()
    else {
        return String::new();
    };
    // `to_lowercase` can change byte lengths, so a byte offset into
    // `body_lower` is not valid for `body`. Recompute the offset in `body`
    // by counting chars, clamping to a char boundary.
    let char_index = body_lower[..lower_offset].chars().count();
    let offset = body
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(body.len());
    let start = body[..offset]
        .rfind(|ch: char| ['.', '\n'].contains(&ch))
        .map(|index| index + 1)
        .unwrap_or(0);
    let end = body[offset..]
        .find(|ch: char| ['.', '\n'].contains(&ch))
        .map(|index| offset + index)
        .unwrap_or_else(|| body.len());
    body[start..end].trim().to_string()
}
```

Note: char-count mapping is an approximation when a single char lowercases to
multiple chars (`İ` → `i̇` is 2 chars), which can shift the excerpt by a few
characters — that is acceptable; panics and wildly wrong excerpts are not.
Do not try to build an exact byte-offset mapping table; it isn't worth it here.

Add unit tests in the existing `mod tests` in `src/search.rs` (see Test plan).

**Verify**: `cargo test --workspace` → exit 0, including the new excerpt tests.

**Commit**: `fix(search): slice note excerpts on char boundaries`

### Step 2: Add `--` before file arguments in `run_optional_tool`

In `src/enforce.rs`, insert `process.arg("--");` immediately before
`process.args(files);` (after the Ruff `check` argument). Both oxlint and
`ruff check` accept `--` as an end-of-options separator.

**Verify**: `cargo test --workspace` → exit 0 (the enforce tests at
`src/enforce.rs:656+` and `tests/cli_workflows.rs` still pass).

**Commit**: `fix(enforce): separate linter options from repo file paths`

### Step 3: Validate the snapshot id shape in `load_snapshot`

In `src/query.rs`, before building the snapshot path, only treat `hash` as a
snapshot filename if it looks like one. Add a small predicate and use the git
fallback otherwise:

```rust
fn is_snapshot_hash(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_hexdigit())
}
```

Restructure `load_snapshot` so that the `.criv/snapshots/{hash}.json` path is
only consulted when `is_snapshot_hash(&hash)` is true; all other ids go
straight to `load_git_state(root, id)`. Behavior that must not change:
- `criv query diff latest latest` still resolves via `.criv/latest`.
- Hex snapshot ids still load from `.criv/snapshots/`.
- Git refs (`HEAD`, branch names) still resolve through `load_git_state`.
- The "snapshot or git ref `<id>` does not resolve" error message still
  appears for nonsense ids.

Add unit tests in the existing `mod tests` in `src/query.rs` (see Test plan).

**Verify**: `cargo test --workspace` → exit 0.

**Commit**: `fix(query): constrain snapshot ids to hash-shaped names`

### Step 4: Full gate

**Verify** (all must pass):
- `cargo fmt --all` then `git diff --stat` shows no unexpected formatting churn
  outside in-scope files
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test --workspace` → exit 0
- `cargo run --quiet -- check` → exit 0

## Test plan

- In `src/search.rs` `mod tests`:
  - `excerpt_handles_multibyte_lowercase_expansion`: body `"İİaé. tail"` with
    query terms `["a"]` — must not panic and must return a non-empty string
    (before the fix this panics on a char boundary).
  - `excerpt_returns_matching_sentence`: ASCII body with two sentences, term
    in the second — returns the second sentence trimmed (guards the existing
    behavior).
- In `src/query.rs` `mod tests`:
  - `snapshot_hash_shape`: `is_snapshot_hash("abc123")` is true;
    `is_snapshot_hash("../../etc/passwd")`, `is_snapshot_hash("HEAD~1")`,
    `is_snapshot_hash("")` are false.
- Model test style on the existing tests in the same `mod tests` blocks.
- Verification: `cargo test --workspace` → all pass including the new tests.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test --workspace` exits 0; the three new tests exist and pass
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `cargo run --quiet -- check` exits 0
- [ ] `grep -n 'body\[..offset\]' src/search.rs` shows the slice now derives
      its offset from `body` (not from `body_lower`'s find result directly)
- [ ] `grep -n 'arg("--")' src/enforce.rs` returns one match inside
      `run_optional_tool`
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The `excerpt()` function at `src/search.rs:442` does not match the excerpt
  above (drift).
- `ruff check --` or `oxlint --` turn out to reject the `--` separator when you
  test locally (then report; do not ship a guessed alternative).
- Fixing `load_snapshot` seems to require changing `load_git_state` or the
  CLI argument definitions.
- Any pre-existing test fails before you make changes (baseline is broken).

## Maintenance notes

- If note excerpts ever move to a proper Unicode-aware search (e.g. caseless
  matching via a crate), delete the char-count mapping and its comment.
- Reviewer should scrutinize: the excerpt test actually exercises a
  multi-byte-expanding character (not just any non-ASCII char).
- Deferred: no attempt to make excerpts grapheme-accurate; only panic-freedom
  and approximate correctness are in scope.

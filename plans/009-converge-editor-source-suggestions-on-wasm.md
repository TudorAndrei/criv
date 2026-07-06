# Plan 009: Give both editors the same source-suggestion ranking via the wasm helper

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- crates/criv-wasm/src/lib.rs .obsidian/plugins/criv/src extensions/vscode-criv/src/stateStore.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (changes suggestion ordering in both editor UIs)
- **Depends on**: plans/012 recommended first if both are selected (it adds
  test seams around the Obsidian `main.ts` code this plan touches); not a hard
  dependency.
- **Category**: tech-debt
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

"Suggest a source file/selector for this query" is implemented twice with
different algorithms: the wasm helper (`crates/criv-wasm`) ranks by substring
containment plus a crude exact/prefix/other rank, and the Obsidian plugin's
TypeScript (`core.ts`) uses a much richer weighted scheme (exact > basename >
suffix > prefix > substring > fuzzy-subsequence, plus frecency). The VS Code
extension uses the wasm ranking; Obsidian uses the TS ranking — so the two
editors return different suggestions for the same state and query, and two
implementations must be maintained. The fix: port the richer TS scoring into
the wasm crate (both editors get the better ranking), route Obsidian through
the wasm function with the existing TS as the documented fallback (matching
the plugin's established `summarizeState` fallback pattern), and lock parity
between wasm and fallback with a shared-fixture test.

## Current state

Relevant files:

- `crates/criv-wasm/src/lib.rs` — the wasm helper. `suggest_source_selectors`
  export (line 44) → `source_selector_suggestions` (lines 105–153) with
  `matches_query` (174) and `selector_rank` (178). Also exports
  `summarize_state`, `source_entries`, `graph_nodes`, `lookup_graph_node`.
  Has a `#[cfg(test)] mod tests` (line 276) with a JSON `editor_state()`
  fixture helper.
- `.obsidian/plugins/criv/src/core.ts` — `sourceEntries` (85, filters every
  path through `safeVaultPath` — a safety behavior wasm's `source_entries`
  does NOT have), `sourceSuggestions` (128–158), `sourceMatchScore` (480),
  `fuzzySubsequenceScore` (502).
- `.obsidian/plugins/criv/src/wasm.ts` — the plugin's wasm loader: exposes
  only `summarizeState`, with a pure-JS fallback used when the wasm module
  fails to load (`loadWasm()` catch → null). This fallback-on-load-failure
  pattern is the plugin's convention — follow it.
- `.obsidian/plugins/criv/src/main.ts` — call sites:
  `sourceEntries(state)` at line 331 (plugin API) and
  `sourceSuggestions(await this.plugin.getState(), context.query, 20)` at
  line 1017 (the `EditorSuggest` autocomplete; it inserts `value.path` and
  renders `value.path`).
- `extensions/vscode-criv/src/stateStore.ts` — VS Code consumption pattern
  (lines 68–102): calls wasm `suggestSourceSelectors(raw, query, limit)`.
- Both extensions build the wasm crate themselves:
  Obsidian `npm run build:wasm` → `wasm-pack build --target bundler`;
  VS Code `npm run build:wasm` → `--target nodejs`. `mise run plugin-build`
  runs the Obsidian build including wasm.

Excerpt — the crude wasm ranking to replace
(`crates/criv-wasm/src/lib.rs:178-190`):

```rust
fn selector_rank(candidate: &str, query: &str) -> usize {
    if query.is_empty() {
        return 0;
    }
    let candidate = candidate.to_lowercase();
    if candidate == query {
        0
    } else if candidate.starts_with(query) {
        1
    } else {
        2
    }
}
```

Excerpt — the richer TS scoring to port (`core.ts:480-516`):

```ts
function sourceMatchScore(path: string, query: string): number | null {
  const lowerPath = path.toLowerCase();
  const basename = lowerPath.split("/").pop() ?? lowerPath;
  if (lowerPath === query) {
    return 100_000;
  }
  if (basename === query) {
    return 90_000;
  }
  if (lowerPath.endsWith(query)) {
    return 80_000 - lowerPath.length;
  }
  if (basename.startsWith(query)) {
    return 70_000 - basename.length;
  }
  if (lowerPath.includes(query)) {
    return 60_000 - lowerPath.indexOf(query) - lowerPath.length;
  }
  const fuzzyScore = fuzzySubsequenceScore(lowerPath, query);
  return fuzzyScore === null ? null : 40_000 + fuzzyScore - lowerPath.length;
}

function fuzzySubsequenceScore(value: string, query: string): number | null {
  let queryIndex = 0;
  let score = 0;
  let run = 0;
  for (let index = 0; index < value.length && queryIndex < query.length; index += 1) {
    if (value[index] !== query[queryIndex]) {
      run = 0;
      continue;
    }
    run += 1;
    score += run * 3 + (index === 0 || value[index - 1] === "/" ? 8 : 0);
    queryIndex += 1;
  }
  return queryIndex === query.length ? score : null;
}
```

Excerpt — the TS entry-point semantics to preserve (`core.ts:128-158`):
empty query → frecency-desc then path-asc, top `limit`; otherwise score
entries (`score + entry.frecency`), drop non-matches (null score), sort by
score desc / frecency desc / path asc, top `limit`.

Shape mismatch you must handle: wasm `suggest_source_selectors` returns
`SourceSelectorSuggestion { target, label, kind, path, detail }` and includes
symbol targets (`path#selector`) from graph nodes; the Obsidian autocomplete
expects `SourceIndexEntry { path, mime?, frecency }` and inserts `value.path`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| wasm crate tests | `cargo test --workspace` | exit 0 |
| Obsidian build (incl. wasm) | `mise run plugin-build` | exit 0 |
| Obsidian tests | `npm --prefix .obsidian/plugins/criv test` | exit 0 |
| Obsidian lint/format | `npm --prefix .obsidian/plugins/criv run lint` / `run format:check` | exit 0 |
| VS Code build (incl. wasm) | `npm --prefix extensions/vscode-criv run build:wasm && npm --prefix extensions/vscode-criv run build` | exit 0 |
| VS Code tests | `npm --prefix extensions/vscode-criv test` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `crates/criv-wasm/src/lib.rs`
- `.obsidian/plugins/criv/src/wasm.ts`
- `.obsidian/plugins/criv/src/main.ts` (only the two call sites + imports)
- `.obsidian/plugins/criv/src/core.ts` (only renaming/documenting the fallback
  role of `sourceSuggestions` — do NOT delete it, see Step 3)
- `.obsidian/plugins/criv/test/core.test.mjs` (parity test)

**Out of scope** (do NOT touch):
- `extensions/vscode-criv/src/**` — it already consumes the wasm functions;
  it picks up the new ranking with zero code change (via its wasm rebuild).
- `safeVaultPath` in either extension.
- The state JSON schema and `src/state.rs`.
- `parseC4Artifact` regions of `core.ts` (plan 008's territory).

## Git workflow

- Conventional commits, suggested sequence:
  `feat(wasm): rank selector suggestions with weighted fuzzy scoring`,
  `refactor(obsidian): route source autocomplete through criv-wasm`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Port the scoring into the wasm crate

In `crates/criv-wasm/src/lib.rs`, replace the ranking inside
`source_selector_suggestions` with a Rust port of `sourceMatchScore` +
`fuzzySubsequenceScore` (byte-for-byte semantic port — same thresholds, same
tiebreaks). Apply the TS entry-point semantics: for file entries use
`score(path) + frecency`; keep including symbol targets from graph nodes
(score them on their `target` string; graph nodes have no frecency — use 0).
Preserve: dedup via the existing `seen` set, empty-query behavior
(frecency desc, then target asc — note the TS empty-query branch sorts by
frecency; symbol nodes with frecency 0 come after files), non-matching
entries dropped (TS returns `null` score → skip), final `truncate(limit)`.

Character-indexing note for the port: the TS loop indexes UTF-16 units;
in Rust iterate `chars()` and track "previous char" for the `/`-boundary
bonus. Paths are overwhelmingly ASCII so minor Unicode scoring differences
between the port and TS are acceptable ONLY if the parity test fixture
(Step 4) stays ASCII — keep it ASCII and note this in the test.

Update the existing wasm unit tests (`selector_suggestions_include_files_and_symbols`
etc., lib.rs:339) for the new ordering and add cases per Test plan.

**Verify**: `cargo test --workspace` → exit 0.

**Commit**: `feat(wasm): rank selector suggestions with weighted fuzzy scoring`

### Step 2: Expose the suggestion function through the plugin's wasm loader

In `.obsidian/plugins/criv/src/wasm.ts`, extend `CrivWasmModule` with
`suggest_source_selectors(raw: string, query: string, limit: number)` and add
an exported wrapper following the exact `summarizeState` pattern:

```ts
export interface CrivSelectorSuggestion {
  target: string;
  label: string;
  kind: string;
  path: string;
  detail: string;
}

export async function suggestSourceSelectors(
  raw: string,
  query: string,
  limit: number,
): Promise<CrivSelectorSuggestion[] | null> {
  const wasm = await loadWasm();
  if (!wasm) {
    return null; // caller falls back to the TS implementation
  }
  return wasm.suggest_source_selectors(raw, query, limit) as CrivSelectorSuggestion[];
}
```

Returning `null` (rather than reimplementing the fallback here) keeps the
fallback logic in `core.ts`, where it is already tested.

**Verify**: `mise run plugin-build` → exit 0 (rebuilds wasm + typechecks).

### Step 3: Route the Obsidian autocomplete through wasm with TS fallback

In `.obsidian/plugins/criv/src/main.ts`, change the `getSuggestions` call site
(line ~1017): try `suggestSourceSelectors(rawState, context.query, 20)` first;
when it returns `null` (wasm unavailable), fall back to the existing
`sourceSuggestions(state, context.query, 20)`.

Practical notes:

- The autocomplete needs the RAW state JSON string for the wasm call. Find how
  the plugin reads state (`readState`/`loadState` around `main.ts:183-235`
  keep the parsed object; check whether the raw string is retained). If only
  the parsed object is available, `JSON.stringify(state)` is an acceptable
  bridge — note it in a comment; do not restructure state loading.
- Map wasm results to the shape the suggest UI consumes: render `label`
  (or `target`) and insert `target` on selection. Suggestion items may now be
  symbol selectors (`src/lib.rs#fn:run`), which is desirable under ADR-0034
  (AST-aware source selectors) — the insert-text becomes `target` instead of
  `path`. Adjust `renderSuggestion`/`selectSuggestion` minimally to use a
  union type or map both sources into one
  `{ insertText: string; label: string }` shape.
- **Safety filter**: wasm results are NOT `safeVaultPath`-filtered (the TS
  `sourceEntries` filter is a plugin-side guarantee). Apply
  `safeVaultPath(item.path)` to each wasm result and drop failures, so the
  routing change cannot weaken the existing guarantee.
- In `core.ts`, add a doc comment on `sourceSuggestions` stating it is the
  fallback for the wasm ranking and that the parity test keeps the two in
  sync. Do not rename exports (the test bundle imports them).

**Verify**:
- `npm --prefix .obsidian/plugins/criv test` → exit 0
- `mise run plugin-build` → exit 0
- `npm --prefix .obsidian/plugins/criv run lint` and `run format:check` → exit 0

**Commit**: `refactor(obsidian): route source autocomplete through criv-wasm`

### Step 4: Parity test between wasm and the TS fallback

The Obsidian test (`test/core.test.mjs`) already bundles `core.ts` and runs in
Node. The Obsidian wasm build targets `bundler`, which plain Node cannot load
— so for the parity test, load the **VS Code** wasm build instead (nodejs
target, `extensions/vscode-criv/pkg/`): both are built from the same crate, so
parity against either build is parity against the crate. Steps:

- Ensure the nodejs wasm build exists in the test run:
  the parity test should `require`/import
  `extensions/vscode-criv/pkg/criv_wasm.js` via a resolved absolute path, and
  SKIP with a printed warning if the pkg is absent (so `plugin-test` doesn't
  hard-depend on the other extension's build artifacts in every environment).
  In CI, `mise run check` builds both extensions, so the parity assertion
  runs there.
- Fixture: a small state JSON (extend the pattern of
  `fixtures/link-resolution.json` or inline it) with ~8 source-index entries
  (varied paths + frecencies, ASCII only) and 2 symbol graph nodes.
- Assert: for queries `""`, `"lib"`, `"main.rs"`, `"src/x"`, and a
  fuzzy-only query (e.g. `"slr"` matching `src/lib.rs` as a subsequence), the
  wasm `suggest_source_selectors(raw, q, 20)` **file** results (kind
  `"file"`) equal `core.sourceSuggestions(state, q, 20)` paths, in order.
  (Symbol suggestions have no TS counterpart — filter them out of the wasm
  side for the comparison.)

**Verify**: `npm --prefix .obsidian/plugins/criv test` → exit 0 with the
parity assertions executed (build the VS Code wasm first:
`npm --prefix extensions/vscode-criv run build:wasm`).

## Test plan

- wasm unit tests (`crates/criv-wasm/src/lib.rs mod tests`): exact-match
  beats basename-match beats suffix beats prefix beats substring beats fuzzy;
  frecency breaks ties; non-matching entries excluded; empty query returns
  frecency order; limit respected.
- Obsidian parity test per Step 4.
- Existing tests in both extensions and the wasm crate stay green
  (`stateStore` tests in VS Code cover its consumption path).
- Verification commands as listed per step.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test --workspace` exits 0 (new ranking tests included)
- [ ] `grep -n 'fuzzy' crates/criv-wasm/src/lib.rs` → the ported scorer exists
- [ ] `grep -n 'suggestSourceSelectors' .obsidian/plugins/criv/src/main.ts` →
      the autocomplete call site uses the wasm path
- [ ] `grep -n 'safeVaultPath' .obsidian/plugins/criv/src/main.ts` (or the
      mapping helper) → wasm results are filtered
- [ ] Obsidian tests include the wasm↔TS parity assertions and pass with the
      nodejs wasm build present
- [ ] `mise run plugin-build`, both extensions' test/lint/format gates, and
      `cargo clippy --workspace --all-targets -- -D warnings` all exit 0
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The wasm module cannot be loaded in the Obsidian runtime for suggestions
  with acceptable latency (the autocomplete is per-keystroke; if wasm-call
  overhead per keystroke is clearly worse than the TS path when you test the
  built plugin, report with numbers — pre-parsing state per keystroke via
  `JSON.stringify`/re-parse may be the bottleneck; a raw-state cache is the
  likely fix but touches state loading, which is out of scope).
- The `EditorSuggest` type constraints in the Obsidian API make the union
  suggestion shape awkward beyond a simple mapping — report rather than
  restructuring the suggest class.
- Parity cannot be achieved on some query class due to UTF-16 vs chars
  scoring differences even with ASCII fixtures.
- You find another consumer of `sourceSuggestions`/`sourceEntries` beyond
  `main.ts:331` and `main.ts:1017` (grep first) whose behavior would change.

## Maintenance notes

- The TS `sourceMatchScore` and the Rust port must now evolve together; the
  parity test is the enforcement. Reviewers of scoring tweaks should demand
  both sides + fixture updates in one PR.
- Deferred: porting `safeVaultPath` filtering into the wasm crate itself
  (would centralize the guarantee for both editors); removing the TS fallback
  entirely once wasm-load reliability in Obsidian is proven (needs telemetry
  or long soak, not guesswork).
- Plan 008 adds C4 golden fixtures in `fixtures/c4/`; if you also add a state
  fixture file here, keep it under `fixtures/` beside `link-resolution.json`.

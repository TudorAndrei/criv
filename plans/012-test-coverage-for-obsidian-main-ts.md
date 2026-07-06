# Plan 012: Bring the Obsidian plugin's main.ts under test (extract seams + stubbed smoke tests)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- .obsidian/plugins/criv`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition. (Plans 003, 008, 009 also touch
> this plugin — read their plan files if the drift is theirs.)

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: LOW (test-focused; production changes are extractions, not
  behavior changes)
- **Depends on**: none (but land BEFORE plans/009 if both are selected — 009
  rewires the autocomplete this plan puts under test)
- **Category**: tests
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

`.obsidian/plugins/criv/src/main.ts` is ~1,500 lines and the highest-churn
TypeScript file in the repo (10 of the last 100 commits), owning state
loading/schema validation, link decoration, source-panel and C4 views, hover
previews, save-command patching, and autocomplete. The plugin test suite
bundles and tests ONLY `core.ts` — no test loads `main.ts` at all, so all of
that behavior regresses silently. Full UI testing needs Obsidian; the
achievable, high-value slice is (a) extracting the pure decision logic into
the already-tested `core.ts` seam and (b) a stub-bundled smoke test that
exercises the plugin class's state-loading contract without Obsidian.

## Current state

Relevant files (all under `.obsidian/plugins/criv/`):

- `src/main.ts` — the plugin. Key regions:
  - `CrivPlugin` class (line 89): `onload` (97) wires commands/views/events;
    `readState` (184), `loadState` (197), `getState` (222), `stateStatus`
    (227), `cachedState` (231), `reloadState` (234).
  - Module-level helpers below the classes: `renderErrorsMessage` (990),
    `linkTargets` (1052), `addTextTargets` (1077), `addTarget` (1091),
    `parseLineRange` (1140), `resolveSourceFromElement` (1032),
    `resolvePatternFromElement` (1042).
- `src/core.ts` — the tested pure module (560 lines, bundled by
  `test/core.test.mjs`).
- `test/core.test.mjs` — the only test; bundles `src/core.ts` with esbuild
  then asserts with `node:assert/strict` (plain script, no framework).
- `package.json` — `"test": "node test/core.test.mjs"` (plan 003 may have
  extended this to run multiple files; append, don't replace).
- `esbuild.config.mjs` — the production bundle marks as external: `obsidian`,
  `electron`, `./pkg/criv_wasm.js`, all `@codemirror/*`, `@lezer/*`, and node
  builtins. `mermaid` and `@viz-js/viz` are real installed deps and are
  bundled.

Excerpt — the state-loading logic to extract and test
(`src/main.ts:197-220`):

```ts
  async loadState(): Promise<CrivState | null> {
    const statePath = this.safeStatePath();
    if (!statePath) {
      this.state = null;
      this.stateError = `Invalid criv state path ${this.settings.statePath}.`;
      return null;
    }
    try {
      const raw = await this.app.vault.adapter.read(statePath);
      const state = JSON.parse(raw) as CrivState;
      if (state.schema !== EXPECTED_SCHEMA) {
        this.state = null;
        this.stateError = `Unsupported criv state schema ${state.schema ?? "unknown"}`;
        return null;
      }
      this.state = state;
      this.stateError = null;
      return state;
    } catch (error) {
      this.state = null;
      this.stateError = `Could not read ${statePath}: ${errorMessage(error)}`;
      return null;
    }
  }
```

`main.ts` imports (lines 1–49): `obsidian`, `mermaid`, `@viz-js/viz`,
`@codemirror/state` (`RangeSetBuilder`), `@codemirror/view`, `./core`,
`./wasm`. The `@codemirror/*` packages are NOT installed (they're
Obsidian-provided externals) — any test bundle of `main.ts` must alias them
to stubs or esbuild will fail to resolve them.

Conventions: oxlint + oxfmt; plain-assert test scripts; conventional commits.

## Commands you will need

Run npm commands with `--prefix .obsidian/plugins/criv` from the repo root.

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `npm --prefix .obsidian/plugins/criv test` | exit 0 |
| Typecheck + bundle | `mise run plugin-build` | exit 0 |
| Lint / format | `npm --prefix .obsidian/plugins/criv run lint` / `run format:check` | exit 0 |

## Scope

**In scope** (the only files you should modify/create):
- `.obsidian/plugins/criv/src/main.ts` (extractions + calling the extracted
  functions — no behavior change)
- `.obsidian/plugins/criv/src/core.ts` (receiving extracted pure functions)
- `.obsidian/plugins/criv/test/core.test.mjs` (tests for the extracted logic)
- `.obsidian/plugins/criv/test/main.test.mjs` (create — the stubbed smoke test)
- `.obsidian/plugins/criv/test/stubs/` (create — obsidian/codemirror stub modules)
- `.obsidian/plugins/criv/package.json` (test script only)

**Out of scope** (do NOT touch):
- `parseC4Artifact` and the DOT sanitizer in `core.ts` (plan 008 territory).
- `src/wasm.ts` and the autocomplete routing (plan 009 territory) — this plan
  tests the CURRENT `sourceSuggestions`-based behavior.
- `extensions/vscode-criv/**`.
- Introducing a test framework or DOM library (jsdom/happy-dom) — plain
  assert scripts only, matching the suite.
- The production `esbuild.config.mjs`.

## Git workflow

- Conventional commits, suggested:
  `refactor(obsidian): extract pure state and link helpers to core`,
  `test(obsidian): smoke-test plugin state loading with stubs`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Extract the pure decision logic into `core.ts`

Move (not copy) these into `core.ts` as exported functions, updating
`main.ts` to import them:

1. **State interpretation** — new `export function interpretState(raw:
   string, expectedSchema: string): { state: CrivState } | { error: string }`
   containing the parse + schema-check logic from `loadState` (the
   `JSON.parse`, the `state.schema !== EXPECTED_SCHEMA` branch with its exact
   error string, and the catch mapping to `Could not read ...` — keep the
   path interpolation in `main.ts` since `core.ts` shouldn't know paths:
   return the message suffix and let `loadState` prepend
   `Could not read ${statePath}: `; design the return shape so `loadState`
   becomes a thin adapter that only does `safeStatePath`, the adapter read,
   and field assignment).
2. `parseLineRange` (main.ts:1140) — pure string → range.
3. `renderErrorsMessage` (main.ts:990) — pure array → string.
4. `addTextTargets` / `addTarget` (main.ts:1077/1091) — pure list ops.

Do NOT extract the DOM-reading helpers (`linkTargets`,
`resolveSourceFromElement`) in this step — they take `HTMLElement`; extract
only if you can split a pure core (e.g. attribute-string → targets) from the
DOM reads within the same effort budget; otherwise leave and note it.

`EXPECTED_SCHEMA`: find where it's defined in `main.ts`; pass it as a
parameter (as sketched) rather than moving the constant, so `main.ts` keeps
owning its config surface.

**Verify**: `mise run plugin-build` → exit 0 (typecheck proves the wiring);
`npm --prefix .obsidian/plugins/criv test` → exit 0.

**Commit**: `refactor(obsidian): extract pure state and link helpers to core`

### Step 2: Test the extracted functions

In `test/core.test.mjs` (or a sibling file added to the test script), assert:

- `interpretState`: valid state round-trips; wrong schema → the exact
  `Unsupported criv state schema <x>` message; `schema` missing → `unknown`
  in the message; invalid JSON → error shape (whatever Step 1's design
  returns for the catch case).
- `parseLineRange`: `"L10"`, `"L10-L20"`, malformed (`"X"`, `""`, `null`) —
  read the implementation first and pin its actual behavior, including the
  0-vs-1-based convention.
- `renderErrorsMessage`: empty array, one error, mixed levels.
- `addTarget`/`addTextTargets`: dedup and null/empty handling.

**Verify**: `npm --prefix .obsidian/plugins/criv test` → exit 0 with the new
assertions executed.

### Step 3: Stub-bundled smoke test for `CrivPlugin` state methods

Create `test/stubs/obsidian.mjs` exporting minimal classes/functions that
`main.ts` imports from `obsidian` (check the import list at `main.ts:1-17`
and stub exactly those: `Plugin` as a class with a constructor storing
`app`, no-op `loadData`, `addRibbonIcon`, `addCommand`, `registerView`,
etc. — only what the CONSTRUCTOR and the state methods you'll call actually
touch; `Notice` as a no-op class; `ItemView`/`FileView`/`PluginSettingTab`/
`EditorSuggest` as empty classes since class-extension requires the symbol
to exist at module evaluation). Create `test/stubs/codemirror.mjs` exporting
no-op `RangeSetBuilder`, `Decoration`, `ViewPlugin`, etc. per the
`@codemirror/state`/`@codemirror/view` imports at `main.ts:20-28`.

Create `test/main.test.mjs` following the `core.test.mjs` esbuild pattern,
with aliasing:

```js
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/main.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
  alias: {
    obsidian: resolve(pluginRoot, "test/stubs/obsidian.mjs"),
    "@codemirror/state": resolve(pluginRoot, "test/stubs/codemirror.mjs"),
    "@codemirror/view": resolve(pluginRoot, "test/stubs/codemirror.mjs"),
  },
  external: ["./pkg/criv_wasm.js", "mermaid", "@viz-js/viz"],
});
```

(`mermaid`/`@viz-js/viz` external keeps the bundle small and load fast; the
state methods under test never touch them. If module-evaluation side effects
still pull something unresolvable, stub it the same way — follow the esbuild
errors.)

Then, WITHOUT calling `onload`, construct the plugin with a fake app and
exercise the contract:

```js
const { default: CrivPlugin } = await import(pathToFileURL(outFile).href);
function fakeApp(files) {
  return { vault: { adapter: { read: async (p) => {
    if (p in files) return files[p];
    throw new Error("ENOENT");
  } } }, workspace: { updateOptions: () => {} } };
}
const plugin = new CrivPlugin(fakeApp({ ".criv/state.json": validRaw }), {});
plugin.settings = { statePath: ".criv/state.json", /* copy DEFAULT_SETTINGS fields the methods read */ };
```

Assert:

- `loadState()` with valid raw → returns the state; `cachedState()` set;
  `stateStatus()` falls back to the "unavailable" message only when error is
  null... (read `stateStatus` — it returns `stateError ?? unavailable`; pin
  actual semantics).
- Missing file → `loadState()` null, `stateStatus()` contains
  `Could not read .criv/state.json`.
- Wrong schema raw → null + `Unsupported criv state schema`.
- Unsafe `statePath` (e.g. `../outside.json`) → null +
  `Invalid criv state path` (this exercises `safeStatePath`).
- `getState()` returns the cached state without re-reading (make the fake
  adapter count reads and assert 1).
- `readState()` with valid raw → a summary object with `node_count` (the
  wasm module falls back to the JS path when `./pkg/criv_wasm.js` can't load
  — mark it external and let the import fail at runtime; `wasm.ts` catches
  and falls back; if the external import instead crashes the bundle at load,
  alias it to a stub that throws on import... check `wasm.ts:loadWasm` — it
  `.catch(() => null)`s the dynamic import, so a missing module is the
  designed fallback path).

Wire into `package.json`: append `&& node test/main.test.mjs` to the test
script.

**Verify**: `npm --prefix .obsidian/plugins/criv test` → exit 0; all three
test files run.

**Commit**: `test(obsidian): smoke-test plugin state loading with stubs`

### Step 4: Gate

**Verify**:
- `npm --prefix .obsidian/plugins/criv run lint` → exit 0 (stubs included —
  add them to the lint globs if oxlint's current `src test ...` args don't
  already cover `test/stubs`)
- `npm --prefix .obsidian/plugins/criv run format:check` → exit 0
- `mise run plugin-build` → exit 0 (production bundle unaffected)
- `git status` clean outside the in-scope list

## Test plan

Steps 2–3 are the test plan. Coverage bar: every branch of
`loadState`/`getState`/`readState`/`stateStatus` and each extracted pure
function has at least one assertion. Structural model: `test/core.test.mjs`.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `npm --prefix .obsidian/plugins/criv test` exits 0 and runs
      `main.test.mjs`
- [ ] `grep -n 'interpretState\|parseLineRange\|renderErrorsMessage' .obsidian/plugins/criv/src/core.ts`
      → all extracted functions present (adjust names to what you chose)
- [ ] `main.ts` imports those from `./core` (grep the import block)
- [ ] The smoke test asserts all four state-loading failure modes (missing
      file, bad schema, bad JSON via interpretState, unsafe path)
- [ ] lint + format:check + `mise run plugin-build` exit 0
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- Bundling `main.ts` with stubs fails on module-evaluation side effects you
  cannot stub within ~5 modules (e.g. deep Obsidian DOM prototype
  augmentation at import time) — report which import chain blocks it; the
  extraction work (Steps 1–2) still stands alone and should be committed.
- Any extraction forces a behavior change (different error string, different
  null-handling) — extraction must be behavior-preserving; report instead.
- `esbuild`'s `alias` option is unavailable in the installed esbuild version
  (check `esbuild.version`; `alias` needs ≥0.17 — the repo has 0.28.x, so
  this should not happen; if it does, the repo's install is broken — report).

## Maintenance notes

- The obsidian stub is intentionally minimal; when a future test needs
  another API, extend the stub — do not import real Obsidian.
- Plan 009 rewires `getSuggestions` through wasm; when it lands, its parity
  test plus these state-loading tests together cover the autocomplete path's
  two halves. If 009 landed first, rebase Step 3's suggestion-related
  assumptions on the new call shape.
- Deferred: DOM-dependent coverage (`decorateLinks`, hover previews, C4 view
  rendering) — needs an Obsidian-flavored DOM; revisit only if regressions
  actually bite there. Save-command patching (`patchNativeSaveCommand`,
  main.ts:117) is also deferred — it reaches into Obsidian command internals
  that a stub would fake too deeply to be meaningful.

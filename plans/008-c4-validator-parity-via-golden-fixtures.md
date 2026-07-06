# Plan 008: Converge the three C4 artifact validators and lock parity with golden fixtures

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/c4_artifact.rs extensions/vscode-criv/src/c4Artifact.ts .obsidian/plugins/criv/src/core.ts fixtures`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/002 (VS Code tests must run in the gate for the new
  parity tests to matter)
- **Category**: tech-debt
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

`.c4` architecture artifacts are validated in three places: the Rust CLI
(`criv check`, the authority), the VS Code extension (editor diagnostics +
preview), and the Obsidian plugin. The two TypeScript copies are byte-identical
to each other, but both have drifted from the Rust authority: editors and CLI
now emit **different diagnostic sets** for the same file, so an author can get
a green editor and a red CI (or vice versa). Full consolidation into one
implementation is blocked — the wasm helper crate cannot depend on the criv
lib (tree-sitter/git2 are not wasm-buildable) — so the achievable fix is:
close the three known semantic gaps, then lock parity with shared golden
fixtures that all three test suites assert against, so future drift fails a
test instead of shipping.

## Current state

The three implementations:

- `src/c4_artifact.rs` — Rust authority. Directive loop ~lines 100–136,
  format inference/mismatch ~138–158, `parse_mermaid_artifact` ~185–225.
- `extensions/vscode-criv/src/c4Artifact.ts` — `parseC4Artifact` at lines
  17–113.
- `.obsidian/plugins/criv/src/core.ts` — `parseC4Artifact` at lines 257–353.
  **Byte-identical to the VS Code copy today** (verified with `diff` at
  planning time) — every change to one must be mirrored to the other,
  and the existing parity tests in both suites must keep passing.

Diagnostic-code inventory (verified at planning time):

| code | Rust | TS (both copies) |
|------|------|------------------|
| `missing-c4-level` | yes (`c4_artifact.rs:92`) | yes |
| `duplicate-c4-format` | yes (`:107`) | **missing** |
| `invalid-c4-format` | yes (`:117`) | yes |
| `unknown-c4-directive` | yes (`:129`) | yes |
| `mismatched-c4-format` | yes (`:143`) | yes |
| `unknown-c4-format` | yes (`:153`) | yes |
| `invalid-c4-mermaid` | yes (`:191`) | yes |
| `mismatched-c4-level` | yes (`:204,:214`) | yes |
| `invalid-c4-level` (DOT must be code-level) | **missing** | yes (`c4Artifact.ts:90`) |
| `invalid-c4-generated` (value must be true/false) | **missing** | yes (`c4Artifact.ts:101`) |

Excerpt — Rust conflicting-format detection the TS copies lack
(`src/c4_artifact.rs:104-114`):

```rust
            "format" => match value.as_deref().and_then(parse_format) {
                Some(format) => {
                    if asserted_format.is_some_and(|existing| existing != format) {
                        diagnostics.push(C4ArtifactDiagnostic {
                            code: "duplicate-c4-format",
                            line: Some(line),
                            message: "conflicting criv:format directives".into(),
                        });
                    }
                    asserted_format = Some(format);
```

The TS copies instead take the first format directive via `.find(...)`
(`c4Artifact.ts:26`, `core.ts:266`) and never inspect later ones.

Excerpt — TS checks the Rust side lacks (`c4Artifact.ts:88-104`, identical in
`core.ts:328-344`):

```ts
  if (format === "dot" && level !== "unknown" && level !== "code") {
    diagnostics.push({
      code: "invalid-c4-level",
      line: null,
      message: "DOT .c4 artifacts are expected to be code-level files.",
    });
  }
  if (
    generatedDirective?.value &&
    generatedDirective.value !== "true" &&
    generatedDirective.value !== "false"
  ) {
    diagnostics.push({
      code: "invalid-c4-generated",
      line: generatedDirective.line,
      message: "criv:generated should be true or false.",
    });
  }
```

In Rust, the `"generated"` directive arm just records the directive with no
value validation (`src/c4_artifact.rs:125`), and the DOT format path returns
without any level check (`:160-166`).

Decision context (already settled — follow, don't re-litigate):

- ADR-0030 (docs/adr/0030-dot-for-generated-code-architecture.md) records DOT
  as the format for generated **code-level** architecture; so the TS
  `invalid-c4-level` check is correct and Rust should adopt it, not the other
  way around.
- Parity target is **diagnostic codes + line numbers**, not message text.
  Message wording may differ between Rust and TS (e.g. Rust says
  "criv:format must be one of: mermaid, mermaid-c4, dot, graphviz", TS says
  "criv:format should be mermaid or dot"). Converging wording is optional
  polish; codes/lines are the contract.
- Note an alias asymmetry to verify while testing: Rust `parse_format` accepts
  `mermaid`, `mermaid-c4`, `dot`, `graphviz`; check what the TS
  `c4FormatFromDirective` accepts and align TS to accept the same alias set
  (a fixture with `criv:format graphviz` will expose this).

Existing tests to model on:

- Rust: `#[cfg(test)] mod tests` in `src/c4_artifact.rs` (check bottom of file).
- VS Code: `extensions/vscode-criv/test/unit/c4Artifact.test.ts`
  (`node:test` + `node:assert/strict`; bundled by esbuild to
  `dist-test/unit/` before running — so `__dirname` at runtime is
  `extensions/vscode-criv/dist-test/unit`).
- Obsidian: `.obsidian/plugins/criv/test/core.test.mjs` (plain assert script;
  bundles `src/core.ts` with esbuild, then imports it; `__dirname` is the
  real `test/` directory).
- Shared fixtures precedent: `fixtures/link-resolution.json` at the repo root
  is already read by the Obsidian test (`core.test.mjs:24`).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Rust tests | `cargo test --workspace` | exit 0 |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| VS Code tests | `npm --prefix extensions/vscode-criv test` | exit 0 |
| Obsidian tests | `npm --prefix .obsidian/plugins/criv test` | exit 0 |
| Both TS lints | `npm --prefix <dir> run lint` / `run format:check` | exit 0 |
| Self-check | `cargo run --quiet -- check` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `src/c4_artifact.rs`
- `extensions/vscode-criv/src/c4Artifact.ts`
- `.obsidian/plugins/criv/src/core.ts` (only the `parseC4Artifact` region and
  its helpers — keep byte-parity with the VS Code copy)
- `fixtures/c4/` (create — `.c4` inputs + `expected.json`)
- `src/c4_artifact.rs` tests, `extensions/vscode-criv/test/unit/c4Artifact.test.ts`,
  `.obsidian/plugins/criv/test/core.test.mjs` (or a new sibling test file
  wired into the plugin `test` script)

**Out of scope** (do NOT touch):
- `crates/criv-wasm/**` — consolidating C4 parsing into wasm is explicitly
  deferred (see Maintenance notes).
- `src/c4.rs`, `src/c4_code.rs` — Mermaid diagram parsing internals and DOT
  generation; only the artifact-level validation in `c4_artifact.rs` is in
  scope.
- `docs/architecture/*.c4` — the repo's real artifacts; if your Rust changes
  make `criv check` flag them, that's a STOP condition, not a reason to edit
  them.
- Rendering/sanitization code in either extension.

## Git workflow

- Conventional commits, suggested sequence:
  `fix(c4): validate dot level and generated values in the cli`,
  `fix(editors): detect conflicting c4 format directives`,
  `test(c4): add cross-implementation golden fixtures`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Add the two missing checks to Rust

In `src/c4_artifact.rs`:

1. **`invalid-c4-generated`**: in the directive loop's `"generated"` arm,
   validate the value like the TS excerpt above (a present, non-empty value
   other than `true`/`false` pushes the diagnostic with the directive's line;
   keep pushing the directive either way, matching the TS behavior of still
   recording it).
2. **`invalid-c4-level`**: after format resolution, when the resolved format
   is DOT and the filename level is known and not code-level, push
   `invalid-c4-level`. Mirror the TS condition
   (`format === "dot" && level !== "unknown" && level !== "code"`) using the
   Rust `C4ArtifactLevel` type — find how `level` is derived from the filename
   (the `missing-c4-level` logic near `:92`) and reuse it. TS uses
   `line: null`; use `line: None`.

Run `cargo run --quiet -- check` on this repo immediately: `docs/architecture/
04-code.c4` is a generated DOT code-level artifact and must NOT be flagged;
`01`–`03` are Mermaid and must not be affected.

**Verify**: `cargo test --workspace` → exit 0; `cargo run --quiet -- check` →
exit 0.

**Commit**: `fix(c4): validate dot level and generated values in the cli`

### Step 2: Add conflicting-format detection to both TS copies

In `extensions/vscode-criv/src/c4Artifact.ts` and mirrored byte-for-byte in
`.obsidian/plugins/criv/src/core.ts`: instead of `.find`-ing the first format
directive, iterate all `format` directives in order; parse each; when a later
directive parses to a *different* format than an earlier one, push
`duplicate-c4-format` at the later directive's line with the same shape as
other TS diagnostics. The effective format stays the LAST parsed directive
(match the Rust semantics in the excerpt: `asserted_format = Some(format)` on
every valid directive). Two directives asserting the SAME format are not a
conflict (Rust: `existing != format`).

While here, align the accepted alias set with Rust's `parse_format`
(`mermaid`, `mermaid-c4`, `dot`, `graphviz`) in the TS
`c4FormatFromDirective` if it differs — Step 4's `graphviz` fixture verifies
this.

**Verify**:
- `diff <(sed -n '/function parseC4Artifact/,/^}/p' extensions/vscode-criv/src/c4Artifact.ts) <(sed -n '/function parseC4Artifact/,/^}/p' .obsidian/plugins/criv/src/core.ts)`
  → empty (copies still identical; adjust the sed ranges if the function
  boundaries differ — the point is the shared region stays in sync)
- `npm --prefix extensions/vscode-criv test` → exit 0
- `npm --prefix .obsidian/plugins/criv test` → exit 0

**Commit**: `fix(editors): detect conflicting c4 format directives`

### Step 3: Create the golden fixtures

Create `fixtures/c4/` with these input files and one expectations file:

- `context-valid.c4` — valid Mermaid `C4Context` with
  `%% criv:format mermaid` → no diagnostics. Name it so filename-level
  inference sees "context" (mirror `docs/architecture/01-system-context.c4`
  naming).
- `conflicting-format.c4` — two `criv:format` directives, `mermaid` then
  `dot` → `duplicate-c4-format` (+ whatever mismatch codes both sides now
  agree on — record what the implementations actually emit, they must match).
- `dot-wrong-level.c4` — DOT content, filename implying container level →
  `invalid-c4-level`.
- `bad-generated.c4` — valid content plus `criv:generated maybe` →
  `invalid-c4-generated`.
- `graphviz-alias.c4` — DOT content with `criv:format graphviz`, code-level
  filename → no format diagnostics.
- `unknown-format.c4` — content that is neither Mermaid C4 nor DOT →
  `unknown-c4-format`.
- `expected.json` — an object keyed by fixture filename, each value an array
  of `{ "code": string, "line": number | null }`, sorted by (code, line).

Ground truth for `expected.json`: run the **Rust** parser on each fixture and
record its output (a quick throwaway Rust test or `cargo run -- check` on a
temp vault works; the directive comment syntax is `%% criv:key value` for
Mermaid and `// criv:key value` for DOT — copy the style used in
`docs/architecture/*.c4`). Line-number conventions may differ (Rust uses
1-based `usize`, TS uses numbers-or-null) — normalize in the test harnesses,
not by fudging expected.json: expected.json records the Rust output; each TS
harness maps its nulls/numbers to that convention and asserts equality. If a
fixture exposes a code/line disagreement you cannot resolve by the Step 1/2
fixes, STOP and report the exact divergence.

### Step 4: Wire the parity assertion into all three suites

- **Rust** (`src/c4_artifact.rs` `mod tests`): a test that iterates
  `fixtures/c4/*.c4` (path relative to crate root via
  `env!("CARGO_MANIFEST_DIR")`), parses each, and asserts the (code, line)
  multiset equals `expected.json`.
- **VS Code** (`test/unit/c4Artifact.test.ts`): same assertion via
  `parseC4Artifact`. Fixture path from the bundled test location:
  `resolve(__dirname, "../../../../fixtures/c4")` (dist-test/unit →
  dist-test → vscode-criv → extensions → repo root). Add an
  `existsSync` assert with a clear message so a wrong path fails loudly, not
  as "0 fixtures, test passed". Iterate the fixtures dynamically — do not
  hardcode the list, so adding a fixture updates all three suites.
- **Obsidian** (`test/core.test.mjs` or a new `test/c4-parity.test.mjs` added
  to the `test` npm script): same assertion via the bundled `core.parseC4Artifact`.
  Fixture path: `resolve(__dirname, "../../../../fixtures/c4")` (test →
  plugin → plugins → .obsidian → repo root — verify by listing the directory
  in the test setup and asserting non-empty).

Note: `parseC4Artifact(relativePath, text)` takes the artifact's path as its
first argument on the TS side and infers level from the filename — pass the
fixture filename so level inference works; make the Rust test derive level the
same way (through its normal `parse_file`/content entry point — read how the
existing Rust tests construct artifacts and match that).

**Verify**:
- `cargo test --workspace` → exit 0 (new Rust parity test passes)
- `npm --prefix extensions/vscode-criv test` → exit 0
- `npm --prefix .obsidian/plugins/criv test` → exit 0
- Mutation check: temporarily edit one entry in `expected.json` to a wrong
  code; all three suites must FAIL; revert.

**Commit**: `test(c4): add cross-implementation golden fixtures`

## Test plan

Covered by Steps 3–4 (the fixtures ARE the tests). Additionally keep every
pre-existing c4 test green in all three suites. The mutation check in Step 4
is mandatory — it proves the harnesses actually read the fixtures.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `grep -n 'invalid-c4-level\|invalid-c4-generated' src/c4_artifact.rs` →
      both codes present
- [ ] `grep -n 'duplicate-c4-format' extensions/vscode-criv/src/c4Artifact.ts .obsidian/plugins/criv/src/core.ts`
      → present in both
- [ ] `fixtures/c4/expected.json` exists; ≥6 `.c4` fixtures
- [ ] All three test suites pass and each contains a parity test reading
      `fixtures/c4`
- [ ] The mutation check (wrong expected.json entry) fails all three suites
- [ ] `cargo run --quiet -- check` exits 0 on this repo (its own `.c4`
      artifacts still validate)
- [ ] Lint/format gates pass for both extensions;
      `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The two TS copies are no longer byte-identical when you start (someone
  edited one — reconcile direction is a human call).
- A fixture exposes a Rust↔TS disagreement beyond the three known gaps
  (duplicate-format, dot-level, generated-value) and the alias set — report
  the divergence table instead of unilaterally changing semantics.
- Step 1's Rust changes flag `docs/architecture/*.c4` in this repo.
- Line-number conventions differ so much that (code, line) equality can't be
  made to hold with a mechanical mapping (e.g. off-by-one on every
  diagnostic) — report the mapping you found; a documented normalization in
  the harness is fine, per-fixture fudging is not.

## Maintenance notes

- **Deferred consolidation**: single-sourcing `parseC4Artifact` (e.g. a
  dependency-light `crates/criv-c4` crate exposed through `criv-wasm` to both
  extensions) was considered and deferred — it needs a workspace/packaging
  decision and changes both extension build pipelines. The golden fixtures
  make that future migration safe: port the implementation, keep the fixtures
  passing. Record this in any future ADR that touches C4 parsing.
- Every new C4 diagnostic added to ANY implementation must come with a
  fixture, or parity rots again — reviewers should reject C4 validation PRs
  without a fixture change.
- The Obsidian/VS Code copies must stay byte-identical until consolidation;
  the Step 2 `diff` verification is worth re-running in review.

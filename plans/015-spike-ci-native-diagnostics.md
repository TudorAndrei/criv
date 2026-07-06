# Plan 015 (spike): Design CI-native diagnostic output (SARIF / GitHub annotations) for check and enforce

> **Executor instructions**: This is a DESIGN SPIKE — the deliverable is a
> written design plus a minimal prototype behind no flag changes to default
> behavior. Follow the steps, honor STOP conditions, and update the status
> row in `plans/README.md` when done.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/check.rs src/enforce.rs .github/workflows/ci.yml`
> On drift, compare "Current state" excerpts against live code first.

## Status

- **Priority**: P3
- **Effort**: M (coarse — spike)
- **Risk**: LOW (additive output format; prototype only)
- **Depends on**: plans/011 recommended first (it pins the current text/JSON
  output as a baseline the new format must not disturb)
- **Category**: direction
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

criv's whole proposition is enforcing docs-vs-code consistency in the
workflow, but in CI its findings surface only as raw text in the job log:
`mise run check` runs `criv check` / `criv enforce --stage ci`, and a
reviewer has to open the log to see what drifted. GitHub renders two formats
natively — workflow-command annotations (`::error file=...,line=...::msg`,
zero-infrastructure) and SARIF (code-scanning UI, needs an upload step and
the `security-events: write` permission). Emitting either would put criv
diagnostics inline on the PR "Files changed" view, turning criv into a
first-class PR gate. The structured data already exists (`--format json` on
`check`); this spike designs the format surface, prototypes the cheapest
path, and scopes the follow-up.

## Current state

- `src/check.rs:24-28` — `enum Format { Text, Json }` (clap `value_enum`);
  `Diagnostic { severity, code: &'static str, path, line: Option<usize>,
  message }` (lines 47–53); `print_json` (1164); `check::run` prints then
  errors if any diagnostic `is_error` (lines 96–106).
- `src/enforce.rs` — NO `--format` option at all (`EnforceOptions` has only
  `--stage`); violations print as text lines (e.g.
  `"{path}:{line}: {adr} policy ... matched ..."` around lines 150–160) plus
  native-tool output passthrough.
- `.github/workflows/ci.yml` — single job, `permissions: contents: read`,
  runs `xvfb-run -a mise run check`; criv executes INSIDE hk inside mise, so
  its stdout still reaches the job log (workflow-command annotations work
  from nested processes — they're just stdout lines — but confirm hk does
  not swallow/prefix stdout in a way that breaks `::error` parsing; that is
  a Step 2 measurement).
- ADR-0022 (docs/adr/0022-hosted-ci-entry-point.md) — read it; the CI entry
  point design is a settled decision this spike must compose with, not
  replace.
- The `Diagnostic` fields map cleanly: SARIF `ruleId` ← `code`, `level` ←
  severity, `physicalLocation` ← path+line; annotations map the same fields.
  `enforce`'s violations/native-tool failures are NOT `Diagnostic`s — part of
  the design question is whether enforce should route through the same
  struct.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Current JSON shape | `cargo run --quiet -- check --format json` | JSON array on stdout |
| Prototype run | `cargo run --quiet -- check --format github` (after Step 3) | `::error/::warning` lines |
| Tests | `cargo test --workspace` | exit 0 |
| Gate | `mise run check` | exit 0 |

## Scope

**In scope**:
- A design report at `plans/reports/015-ci-diagnostics-spike.md`.
- A minimal prototype: ONE new format variant on `check` only (recommend
  `github` annotations — no CI permission changes, no upload step), plus a
  unit test of the line encoding. Keep the diff small enough to throw away.

**Out of scope** (design-only, no implementation in this spike):
- SARIF emission (design it in the report; implementing the schema is the
  follow-up).
- Any `enforce` refactor to a shared Diagnostic pipeline (design question,
  not spike code).
- CI workflow changes (`ci.yml` edits, permissions) — describe them in the
  report.
- Changing default output or exit-code behavior.

## Git workflow

- Suggested commits: `feat(check): prototype github annotation output` (the
  prototype, clearly marked in the report as replaceable) and
  `docs(plans): record ci diagnostics design`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Inventory the diagnostic surfaces

Enumerate every finding type that should reach a PR: `check` diagnostics
(codes: grep `code: "` in `src/check.rs` — wiki-links, frontmatter, C4,
markdown-format, policy-violation...), `enforce` policy violations, and
native-tool (oxlint/ruff) failures. For each: does it carry path+line today,
and does it route through `Diagnostic`? Produce the table for the report.

### Step 2: Verify annotations survive the hk/mise nesting

Add a temporary `echo "::warning file=README.md,line=1::criv spike probe"`
to a scratch branch's CI (or reason from hk's output handling by reading how
hk streams step stdout — check `hk.pkl` steps' `check_first`/output docs and
one real CI log). The report must state definitively whether `::`-commands
emitted by `criv` inside `hk check --all` reach the GitHub parser, with
evidence. If they don't, the design pivots to a wrapper step
(criv writes a file, a workflow step cats it) — design that instead.

### Step 3: Prototype `--format github` on `check`

Add `Github` to the `Format` enum and a `print_github(diagnostics)` that
emits one line per diagnostic:
`::error file=<path>,line=<line>,title=criv <code>::<message>` (warning
severity → `::warning`; omit `,line=` when `line` is `None`; escape per the
GitHub workflow-command rules: `%` → `%25`, `\r` → `%0D`, `\n` → `%0A` in
the message). Keep text/json untouched. Unit-test the encoding (empty line,
special chars) next to the existing print tests in `src/check.rs`'s
`mod tests`.

**Verify**: `cargo run --quiet -- check --format github` on this repo (add a
deliberate temp doc error to see one line, then remove it);
`cargo test --workspace` → exit 0.

### Step 4: Write the design report

`plans/reports/015-ci-diagnostics-spike.md` covering: the Step 1 table; the
Step 2 evidence; annotations-vs-SARIF trade-offs for criv specifically
(annotation cap ~10 per step / 50 per job, no persistent code-scanning UI;
SARIF needs `security-events: write` and an upload action — cite the current
`permissions: contents: read` posture); the recommended rollout (likely:
land `--format github`, wire an `enforce` equivalent AFTER deciding whether
enforce adopts `Diagnostic`, defer SARIF until someone wants the
code-scanning UI); and the follow-up plan outline including the `hk.pkl`/CI
wiring (which step invokes criv with the new format — note hk steps are
shared between local and CI runs, so the format probably needs to key off
`GITHUB_ACTIONS` env or a dedicated hk step, and note that
`tests/cli_workflows.rs` explicitly strips `GITHUB_ACTIONS` env — find why,
`grep -n GITHUB_ACTIONS src/ tests/`, and reflect what you learn: criv may
already have CI detection).

**Verify**: report committed; prototype gated by tests; `mise run check` →
exit 0.

## Test plan

The prototype's encoding unit tests (Step 3). Everything else is the report.

## Done criteria

- [ ] `plans/reports/015-ci-diagnostics-spike.md` committed with the surface
      inventory, nesting evidence, trade-off analysis, recommendation, and
      follow-up outline
- [ ] `--format github` prototype exists on `check` only, default behavior
      unchanged (`cargo run --quiet -- check` output identical to before)
- [ ] Encoding unit tests pass; `mise run check` exits 0
- [ ] `plans/README.md` status row updated

## STOP conditions

- Step 2 shows annotations cannot reach GitHub's parser from inside hk AND
  the file-wrapper alternative would require restructuring hk steps — report
  and stop; the design decision escalates to the maintainer.
- You find existing CI-detection or annotation logic in criv (the
  `GITHUB_ACTIONS` env handling hints something exists) — reconcile with it
  in the report rather than adding a parallel mechanism.

## Maintenance notes

- If the maintainer accepts the design, the follow-up plan should also
  decide `enforce --format` and whether policy violations unify with
  `Diagnostic` — that refactor interacts with plan 006/007's touched code.
- Plan 011's CLI tests pin current text output; the new format must be
  purely additive against that baseline.

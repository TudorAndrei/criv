# Plan 013: Add a root AGENTS.md and a `criv query` reference doc

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- README.md docs/ src/query.rs`
> If `src/query.rs`'s dispatch changed since this plan was written, enumerate
> the live subcommands (`grep -o '"[a-z-]*" =>' src/query.rs`) and document
> those, not this plan's table.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx + docs
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

Two discoverability gaps: (1) there is no root `AGENTS.md`/`CLAUDE.md`, so a
contributor or coding agent must reconstruct the verification workflow from
three files (`hk.pkl`, `mise.toml`, `docs/tooling.md`) and may not learn that
`mise run check` is the umbrella gate; (2) `criv query` is a headline feature
whose actual subcommand tokens are only discoverable by reading
`src/query.rs` — the README describes them in prose that doesn't match the
tokens ("citations" vs `cites`/`cited-by`, "ADR governance" vs
`governs`/`governing`, "orphaned docs" vs `orphan-docs`), and no doc
enumerates the flags (`--by`, `--kind`, `--without-docs`, `--format`).

## Current state

- No `AGENTS.md` or `CLAUDE.md` exists at the repo root (verified).
- `docs/tooling.md` — the deep tooling doc (hooks policy, mise tasks, perf
  script, plugin/extension tasks). AGENTS.md should POINT here, not duplicate
  it.
- The verification commands (validated during planning):
  - Umbrella gate: `mise run check` (runs the hk `check` hook — Rust
    fmt/clippy/test, actionlint, zizmor, both extensions' lint/tests,
    `criv check`, `criv enforce --stage ci`).
  - Per-surface: `cargo test --workspace`,
    `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo run --quiet -- check`, `npm --prefix .obsidian/plugins/criv test`,
    `npm --prefix extensions/vscode-criv test`, `mise run fix`.
- The query dispatch inventory (from `src/query.rs:38-99`): `next-adr-id`,
  `callers <symbol>`, `callees <symbol>`, `attack-surface`,
  `targets <note-id>`, `cites <note-id>`, `cited-by <note-id>`,
  `orphan-docs`, `references <symbol>`, `governs <ADR-ID>`,
  `governing <symbol>`, `coverage [--by <x>]`,
  `nodes [--kind <k>] [--without-docs]`, `c4-elements <note-id>`,
  `c4-relationships <note-id>`, `c4-code <path-glob>`,
  `diff <ref-a> <ref-b>`. Global flag: `--format text|json`.
  Verify each against the live dispatch and each subcommand's `required_arg`
  label (the `<...>` names above come from the `required_arg` calls — use
  those exact labels).
- README query section: lines 148–160 (examples) and the prose at 18–21.
- Vault rules that apply to NEW docs: files under `docs/` are vault notes —
  `criv check` validates their Markdown formatting and frontmatter. Match the
  frontmatter shape of `docs/tooling.md` (open it and copy its
  `id/kind/title/tags` fields pattern). Root-level `.md` files (README,
  AGENTS.md) are NOT vault notes but ARE rumdl-format-checked by the
  `criv-check` hook step (`hk.pkl` glob `**/*.md`).
- ADR-0010/0011: `criv init` installs *agent runtime skills* into vaults —
  that's guidance for agents USING criv in a target repo. AGENTS.md here is
  guidance for agents DEVELOPING criv. Say so in AGENTS.md's first lines to
  keep the two audiences from blurring.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Validate docs | `cargo run --quiet -- check` | exit 0 |
| Full gate | `mise run check` | exit 0 |
| Verify query tokens | `cargo run --quiet -- query <token> ...` | matches the doc |

## Scope

**In scope** (files to create/modify):
- `AGENTS.md` (create, repo root)
- `docs/query-reference.md` (create)
- `README.md` (only: fix the prose/token mismatch at lines ~18–21 and link
  the new reference from the query examples section)

**Out of scope** (do NOT touch):
- `docs/tooling.md`, `docs/releasing.md` — link to them, don't edit.
- `src/query.rs` — if a subcommand behaves differently than documented
  prose suggests, document reality.
- `assets/skills/**`, `.agents/**` — the runtime-skill templates are a
  different audience.
- Generating docs from `criv --usage` — the README already documents that
  path for CLI-wide docs; this plan is a hand-written focused reference
  (see Maintenance notes for the generated-docs alternative).

## Git workflow

- Conventional commits, suggested:
  `docs(agents): add contributor entrypoint for agents`,
  `docs(query): add query subcommand reference`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Write `AGENTS.md`

Keep it under ~60 lines. Required content, in order:

1. One-paragraph orientation: criv is a Rust CLI (`src/`) validating a
   `docs/` vault, with two TypeScript companions (`.obsidian/plugins/criv`,
   `extensions/vscode-criv`) and a wasm helper (`crates/criv-wasm`); this
   repo is itself a criv vault, so docs changes are validated by the tool.
   Note the audience split vs the installed runtime skills (see Current
   state).
2. The verification table: the umbrella `mise run check`, then the
   per-surface commands from Current state, each with one line on when to
   use it. State plainly: "run `mise run check` before finishing any change;
   it is what CI runs."
3. Conventions in five bullets or fewer: conventional commits (give one real
   example from `git log`), `.criv/` is generated and gitignored, docs under
   `docs/` need frontmatter and pass `criv check`, ADRs are immutable once
   accepted (ADR-0012) — new decisions get new ADRs, plans live in `plans/`.
4. Pointers: `docs/tooling.md` (toolchain details), `docs/releasing.md`
   (releases), `docs/adr/README.md` (decision index).

**Verify**: `cargo run --quiet -- check` → exit 0 (rumdl formatting on the
new root file).

### Step 2: Write `docs/query-reference.md`

Frontmatter matching `docs/tooling.md`'s shape. Content: a table of every
query subcommand — token, positional argument (exact `required_arg` label),
flags, one-line description, one runnable example. Order the rows as the
dispatch does. Document `--format text|json` once, above the table. For each
subcommand, RUN it against this repo first (`cargo run --quiet -- query ...`)
and make the example one that produces real output here (e.g.
`criv query governs ADR-0005`).

**Verify**: `cargo run --quiet -- check` → exit 0 (frontmatter + links
validate); every documented example actually ran during writing.

### Step 3: Fix the README mismatch

In the README's What/How sections: make the prose name the real tokens
(e.g. "citations (`cites`, `cited-by`)") or simply link to
`docs/query-reference.md` after the example block. Smallest diff that removes
the mismatch wins.

**Verify**: `mise run check` → exit 0.

**Commits**: as listed in Git workflow.

## Test plan

No code tests. The gate is `criv check` validating the new/edited Markdown
plus manual execution of every documented example during Step 2.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `AGENTS.md` exists at the root and contains `mise run check`
- [ ] `docs/query-reference.md` exists; every token from
      `grep -o '"[a-z-]*" =>' src/query.rs` appears in it
- [ ] README names or links the real tokens (`grep -n 'cites\|query-reference' README.md`)
- [ ] `mise run check` exits 0
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `criv check` rejects the new doc's frontmatter after two attempts at
  matching `docs/tooling.md`'s shape — report the diagnostics.
- A documented example produces output that looks wrong (that's a bug for
  plan 011's territory — report, still document the actual behavior).

## Maintenance notes

- The query table can drift from `src/query.rs`. Two mitigations to consider
  later (not now): a test that greps the dispatch tokens and asserts each is
  in the doc, or switching to `criv --usage`-generated CLI docs (ADR-0019
  records the usage-spec export) — if generated docs land, retire the
  hand-written table in favor of generated output plus prose.
- When plans 001–012 change commands or behavior, AGENTS.md's table is cheap
  to keep current — reviewers should treat it as part of the change surface.

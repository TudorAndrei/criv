# Plan 014 (spike): Decide the fate of the dark `embeddings` feature — ship a variant or retire it

> **Executor instructions**: This is a DESIGN SPIKE, not a build plan. The
> deliverable is a written recommendation with measurements, committed as a
> report — production code changes are limited to what's needed to measure.
> Follow the steps, honor STOP conditions, and update the status row in
> `plans/README.md` when done.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/search.rs Cargo.toml .github/workflows/release.yml`
> On drift, compare "Current state" excerpts against live code first.

## Status

- **Priority**: P3
- **Effort**: M (coarse — spike)
- **Risk**: LOW (measurement only; the decision itself is the deliverable)
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

criv ships a complete semantic note search (`criv search --notes` semantic
path: fastembed init, model download to `.criv/embeddings`, cosine ranking)
that **no distributed build can run**: the cargo feature `embeddings` is off
by default, the release workflow builds with no `--features` flag, and
`cargo install --git` (the README's install path) likewise gets the default
features. ADR-0008 records semantic search as an *optional* capability, but
today it is not optional-off, it is unreachable — pure maintenance weight
(a code path plus the fastembed dependency tree in `Cargo.toml`) unless a
build actually enables it. The maintainer needs data to decide: ship an
embeddings-enabled variant, document a from-source opt-in, or retire the
code path. This spike produces that data and a recommendation.

## Current state

- `Cargo.toml:21-23` — `default = []`, `embeddings = ["dep:fastembed"]`;
  fastembed pinned with rustls features (lines 38–41).
- `src/search.rs:264-316` — `#[cfg(feature = "embeddings")] fn
  semantic_notes(...)` (fastembed `AllMiniLML6V2`, cache dir
  `.criv/embeddings`, downloads the model on first use); the
  `#[cfg(not(feature = "embeddings"))]` twin returns the error
  "semantic note search requires building criv with `--features embeddings`".
- `.github/workflows/release.yml:52` —
  `cargo build --locked --release --target ... --bin criv` (no features) —
  across a 4-target matrix (linux x86_64/aarch64, macOS aarch64, windows
  x86_64).
- `criv.toml` `[index] embeddings = false` in this repo; check how
  `src/config.rs` consumes that flag and how `search.rs` decides lexical vs
  semantic (find the call site of `semantic_notes` — what CLI flag or config
  triggers it).
- ADR-0008 (docs/adr/0008-optional-semantic-note-search.md) — read it fully
  before writing the recommendation; the decision must be framed as
  continuing or superseding it (ADR-0012: accepted ADRs are immutable — a
  changed decision needs a NEW ADR).
- Release profile is size-optimized (ADR-0015, `Cargo.toml:14-19`) — binary
  size is an explicit project value.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Baseline build | `cargo build --release --bin criv` | exit 0 |
| Embeddings build | `cargo build --release --features embeddings --bin criv` | exit 0 (or a finding) |
| Size compare | `ls -la target/release/criv` after each | numbers for the report |
| Behavior probe | `cargo run --features embeddings -- search --notes "<query>"` in a scratch vault | works; note cold-start time and download size |
| Tests with feature | `cargo test --workspace --features embeddings` | exit 0 (or a finding) |

## Scope

**In scope**:
- Measurements and a written report at `plans/reports/014-embeddings-spike.md`
  (create the directory).
- Throwaway local builds. A scratch vault under the session temp dir.

**Out of scope** (do NOT do in this spike):
- Changing `Cargo.toml` defaults, the release workflow, or deleting the
  feature — that's the follow-up plan after the maintainer decides.
- Writing the superseding ADR (draft its outline in the report instead).
- Committing any model files or `.criv/embeddings` content.

## Git workflow

- Single commit of the report:
  `docs(plans): record embeddings feature spike findings`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Map the reachable surface

Read `src/search.rs` around the `semantic_notes` call site and `src/config.rs`
for the `[index] embeddings` flag: document exactly what a user must do today
to reach semantic search (build flag + config + CLI invocation). Confirm the
error message path is what non-feature builds hit.

### Step 2: Measure the embeddings build

Build both variants (commands above) and record: compile time delta, binary
size delta (bytes and %), and `cargo tree --features embeddings -d` new
duplicate/native deps. If the feature no longer compiles (it's dark — rot is
plausible), that is itself a headline finding: record the errors and skip to
Step 4 with "retire or repair" framing.

### Step 3: Measure the runtime UX

In a scratch vault with ~20 notes: first `search --notes` run with the
feature (model download size, wall time, where it downloads from), then warm
runs (latency vs lexical search on the same queries). Note result-quality
anecdotes (does semantic actually beat lexical on 3–5 realistic queries?).
Offline behavior: run once with network blocked/unplugged if feasible and
record the failure mode.

### Step 4: Write the recommendation

`plans/reports/014-embeddings-spike.md` with: the measurements table; the
three options (A: add an embeddings-enabled artifact to the release matrix —
enumerate which targets and the workflow change; B: keep source-only opt-in
but document it honestly in README + a `search --notes` hint; C: retire the
feature — delete `semantic_notes`, the fastembed dep, and supersede ADR-0008)
each with one-paragraph costs; a single recommended option with rationale
grounded in the numbers; and the outline of the new ADR the decision needs.
Cite ADR-0008's stated intent when arguing.

**Verify**: report exists, contains the measurements table and exactly one
recommendation; `git status` shows only the report (and `plans/README.md`)
changed.

## Test plan

Not applicable (spike). The report's numbers must be reproducible from the
commands you record in it.

## Done criteria

- [ ] `plans/reports/014-embeddings-spike.md` committed with: build results
      (or compile-failure evidence), size/time deltas, runtime UX
      measurements, three options, one recommendation, ADR outline
- [ ] No changes to `Cargo.toml`, `src/`, or workflows
- [ ] `plans/README.md` status row updated (DONE = report delivered; the
      follow-up implementation is a NEW plan after the maintainer decides)

## STOP conditions

- The embeddings build needs network access to build (not just to run) in a
  way your environment blocks — record what you could measure and report.
- Anything in the spike tempts you to "just fix" production code — don't;
  note it in the report.

## Maintenance notes

- Whichever option is chosen, the decision belongs in a new ADR referencing
  ADR-0008. Option C also has a small blast radius in `criv.toml` parsing
  (`[index] embeddings`) — note in the report whether that config key must
  stay accepted for compatibility.

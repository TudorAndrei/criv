# Plan: Skill staleness nudge and `--force-skills` refresh

## Goal

`criv init` writes the twelve agent skill files create-only, so a vault that has
already been initialized can never receive a corrected or extended skill. Every
skill fix ships to new vaults only, and nothing tells anyone. This closes GitHub
issue 10 by making staleness *visible* — `criv check` says so on every run — and
giving the user one command to act on it, without criv ever rewriting a tracked
tree behind their back.

It ships with #11 (refresh the skill content against the 0.7.0 CLI) so the
mechanism has real content to deliver on its first run, and #2 (re-sync this
repo's drifted `.claude/skills`) is then fixed by running the new command rather
than by hand.

## Approach

### Skills are criv-owned

The governing rule, settled during planning: **users never edit criv skills.**
They are generated artifacts like `.criv/state.json`, not a starting point to
customize. That dissolves what would otherwise be the hardest part of the design
— there is no local edit to protect, no opt-out marker, and no ambiguity about a
file whose body differs from the template.

What it does *not* license is silent rewriting. The skill files are committed to
git, so mutating them during a `criv init` run people execute for other reasons
would produce surprise working-tree changes. Hence: nudge, then an explicit
refresh.

### Detection: a content hash in frontmatter

Each generated `SKILL.md` carries a blake3 hash of the template it came from:

```yaml
---
name: criv
description: Use when working in a criv vault…
metadata:
  criv-template: blake3:7f3a9c2e
---
```

`criv check` hashes each embedded template, reads the marker off the installed
file, and reports a mismatch.

Two alternatives were rejected. A **hand-bumped version** can be forgotten, which
reintroduces the exact bug this closes — a skill fix that never reaches vaults.
The **criv binary version** makes every release mark every vault stale even when
no skill changed, which turns the notice into noise people learn to ignore. A
content hash cannot be forgotten and cannot produce a false nudge: change the
text and every vault sees it, change nothing and nobody is disturbed.

The marker nests under `metadata` because the `skills` CLI spec (vercel-labs)
names `metadata` as the sanctioned optional frontmatter key — it defines
`metadata.internal` — so this stays interoperable with `npx skills add` and the
Claude Code plugin loader. A missing marker means stale, which is correct: every
vault in existence today has no marker and is by definition on an old template.

blake3 is already a dependency (`Cargo.toml:35`, used in `src/source_graph.rs`,
`src/state.rs`, `src/source_index.rs`), so this adds nothing to the tree.

### The nudge

Printed by `criv check` on **every run**, after the diagnostics:

```text
criv check: ok
note: 3 agent skills are out of date; run `criv init --force-skills`
```

Three constraints, all non-negotiable:

- **Text format only.** `print_json` (`src/check.rs:1254`) and `print_github`
  (`src/check.rs:1261`) must stay byte-identical. The VS Code extension feeds
  `check --format json` stdout straight to `JSON.parse` (`extension.ts:118`);
  an extra line there is the failure mode of open issue #8.
- **Never affects exit status.** It is emitted outside the
  `diagnostics.iter().any(Diagnostic::is_error)` branch at `src/check.rs:110`.
  It is a note, not a diagnostic, and does not flow through `--filter`.
- **Silent when there is nothing to say**, including in a vault initialized with
  `--no-skills`, where no skill files exist at all.

`criv check` runs three times per commit through the generated pre-commit hook,
alongside `watch --once` and `enforce`. Nudging from `check` alone keeps that to
one notice per commit rather than three.

### The refresh

`criv init --force-skills` mirrors the existing `--force-hooks` precedent
(`src/init.rs:31`, `write_hook` at `src/init.rs:271`): a bool on `InitOptions`,
threaded to the skill writes, overwriting where they currently skip. It reuses
`write_template`'s confinement — `write_new_in` → `prepare_confined_write` — so
root confinement, symlink rejection, and relative-path validation are unchanged
per ADR-0044.

Reporting follows the existing `created` / `hook_messages` shape at
`src/init.rs:96-107`, naming each refreshed file.

### Out of scope

- Auto-updating skills without the flag.
- A `criv skills` subcommand tree.
- Any protection for locally edited skills — by the rule above, there are none.
- Rate-limiting the nudge. It fires every run by decision; if that proves
  annoying in practice, revisit with evidence.

### Decision record

ADR-0010 mandates create-only and states it as a deliberate consequence. It is
accepted and therefore immutable under ADR-0012, so this needs a **superseding
ADR**, not an edit. Use `criv query next-adr-id` — do not guess the number.

Worth quoting in the new ADR: ADR-0010's closing paragraph says future skill
changes "should be made in the initializer template and then generated through
`criv init`", which the create-only rule makes impossible. The ADR is internally
contradictory, and that is the strongest argument for superseding it.

## Implementation Phases

### Phase 1: Template hashing and the frontmatter marker

- Add a `template_hash(contents: &str) -> String` helper in
  `src/init/templates.rs` returning a short blake3 hex digest.
- Add a function that injects `metadata.criv-template` into a template's YAML
  frontmatter at write time, so `assets/skills/*/SKILL.md` stay marker-free in
  the repo and the marker is generated. Reuse the frontmatter delimiter logic
  shape from `split_frontmatter` (`src/vault.rs:516`) rather than a regex.
- Unit tests: hash is stable for identical input and differs for changed input;
  injection produces valid YAML for a skill that already has `name` and
  `description`; injecting twice is idempotent.
  **Commit:** `feat(init): stamp generated skills with a template hash`

### Phase 2: `--force-skills`

- Add `force_skills: bool` to `InitOptions` (`src/init.rs:20-32`).
- Thread it into the skill template writes at `src/init.rs:71-73`; when set,
  overwrite instead of skipping, still through the confined write path.
- Report refreshed files alongside `created`, following `src/init.rs:96-107`.
- Tests in `src/init/tests.rs`: a stale skill is untouched without the flag and
  refreshed with it; a fresh vault is unaffected by the flag; a symlinked skill
  destination still errors (confinement unchanged).
  **Commit:** `feat(init): add --force-skills to refresh installed skills`

### Phase 3: The nudge in `criv check`

- Add a function that compares each installed skill's marker against the
  embedded template hash and returns the count plus paths.
- Emit the note from `run` (`src/check.rs:72`) in the `Format::Text` arm only,
  after `print_text`, outside the error branch at `src/check.rs:110`.
- Tests: nudge appears when a marker is stale, when a marker is missing, and not
  at all when current; `--format json` output parses as JSON with a stale skill
  present; exit status is unchanged by the nudge; a `--no-skills` vault is
  silent.
  **Commit:** `feat(check): report out-of-date agent skills`

### Phase 4: Supersede ADR-0010

- `criv query next-adr-id` for the number.
- Write the ADR: create-only made skill fixes undeliverable, ADR-0010
  contradicts itself, skills are criv-owned so there are no edits to protect,
  and the nudge-not-force choice exists because the files are tracked by git.
- `supersedes: [ADR-0010]`, `governs: [src/init.rs]`.
  **Commit:** `docs(adr): supersede ADR-0010 with the skill refresh path`

### Phase 5: Refresh the skill content (#11)

- Update `assets/skills/*/SKILL.md` against the 0.7.0 surface: `criv query
  next-adr-id` into `writing-decisions`, the three `c4-*` queries into
  `c4-authoring`, `check --fix` and `--filter` into `checking-drift`, and a
  `docs/query-reference.md` pointer into `criv`.
- Keep each skill short; the current brevity is deliberate.
  **Commit:** `docs(skills): document the 0.7.0 CLI surface in agent skills`

### Phase 6: Re-sync this repo (#2)

- Run `cargo run -- init --force-skills` in this repo to refresh
  `.agents/skills` and `.claude/skills` from `assets/skills`.
- Add `.claude/skills` to `criv.toml`'s `[source] roots` so criv can see its own
  third copy.
- Add a test asserting the three trees are byte-identical modulo the marker, so
  the drift cannot silently recur.
  **Commit:** `fix(skills): re-sync installed skills with the shipped templates`

## Risks & Tradeoffs

- **Nudging every run may become noise.** It fires on every commit until acted
  on. That was chosen deliberately over rate-limiting for visibility. Mitigation
  is that acting on it is one command; if it grates, add a rate limit later with
  real evidence rather than guessing now.
- **The marker changes the SKILL.md format.** Nesting under `metadata` follows
  the `skills` CLI's sanctioned extension point, but any consumer that validates
  frontmatter strictly could object. Verify against `npx skills` after Phase 1.
- **A refresh silently discards local edits.** Correct under the criv-owned
  rule, and surprising to anyone who did not know it. The new ADR must state the
  rule explicitly so the behavior is documented rather than discovered.
- **Phase 5 rewrites agent-facing prose** and is the least mechanical part. It
  should be reviewed as writing, not as code.
- **Hash churn in diffs.** Every skill content change now also changes a hash
  line in twelve installed files. That is the mechanism working, but it makes
  skill diffs noisier.

## Resolved Questions

- The nudge reports only the stale count and the refresh command. Listing all
  twelve paths would dominate normal `check` output.
- A missing skill remains silent: `--no-skills` is an intentional choice, not
  staleness.

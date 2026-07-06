# Plan 016 (spike): Design the promised editor-extension install path for `criv init`

> **Executor instructions**: This is a DESIGN SPIKE — the deliverable is a
> written design with a working proof-of-concept command sequence, not a
> shipped feature. Follow the steps, honor STOP conditions, and update the
> status row in `plans/README.md` when done.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/init.rs README.md extensions/vscode-criv/package.json`
> On drift, compare "Current state" excerpts against live code first.
> (Plan 010 rewrites init's git handling — unrelated to this spike's scope
> but expect diffs in `src/init.rs` if it landed.)

## Status

- **Priority**: P3
- **Effort**: S–M (coarse — spike)
- **Risk**: LOW (spike); the eventual feature is MED (spawning editor CLIs is
  environment-dependent)
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

`criv init` scaffolds everything else automatically — hooks, agent skills,
the Obsidian plugin — but for VS Code/Cursor it only writes a
recommendation file, and the README explicitly promises more: "A future
explicit install path can shell out to a selected editor CLI such as `code`
or `cursor` with a published extension ID or local `.vsix`" (README lines
99–103). The manual procedure already exists and is documented
(`docs/tooling.md`: `code --install-extension` / `cursor
--install-extension` for smoke tests), and the extension builds a local
`vscode-criv.vsix` with a stable ID `criv.vscode-criv`. This spike settles
the design questions (flag shape, ID-vs-VSIX source, multi-editor detection,
failure UX) so the feature can be built without relitigating them.

## Current state

- `src/init.rs` — `run` (line 32) orchestrates scaffolding;
  `write_vscode_extension_recommendation` (line 100) writes
  `.vscode/extensions.json`. Flags today: `--no-skills`, `--no-obsidian`,
  `--no-vscode`, `--no-hooks`, `--force-hooks` (check the `InitOptions`
  derive for the exact clap shape to extend).
- `extensions/vscode-criv/package.json` — `"package": "vsce package --out
  vscode-criv.vsix"`; extension ID `criv.vscode-criv` (publisher `criv`).
  **Not published to any marketplace yet** (docs/tooling.md: "MVP
  verification does not require a marketplace token or an Open VSX publish
  step") — so "install by published ID" does not work TODAY; the honest
  near-term source is a local `.vsix` (which a target repo won't have) or
  waiting on publication. This tension is the core design question.
- `docs/tooling.md` (lines ~95–103) — documents the manual VSIX install for
  smoke tests.
- ADR-0035 (docs/adr/0035-vscode-compatible-companion-extension.md) — the
  extension targets VS Code AND derivatives (Cursor et al.); the install
  design must not hardcode one editor.
- Precedent for subprocess patterns: `src/enforce.rs:566+`
  (`run_optional_tool` — tool-on-PATH detection + graceful skip) is the
  in-repo pattern for "shell out to an optional external tool, skip cleanly
  when absent". Model the design on it.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Editor CLI probe | `code --version`, `cursor --version` | version or not-found |
| Manual install (PoC) | `code --install-extension <path-to-vsix>` | "successfully installed" |
| List installed | `code --list-extensions` | contains `criv.vscode-criv` |
| Build the vsix | `mise run vscode-package` | `vscode-criv.vsix` created |

## Scope

**In scope**:
- A design report at `plans/reports/016-editor-install-spike.md`.
- Hands-on PoC: build the VSIX, install/uninstall it into at least one
  locally available editor CLI, record exact commands, output, and exit
  codes (including the failure cases: no editor on PATH, bad VSIX path).

**Out of scope** (no implementation in this spike):
- Changes to `src/init.rs` or new CLI flags.
- Publishing the extension to VS Code Marketplace / Open VSX (a separate
  decision with account/token implications — the report should scope it as
  the likely prerequisite).
- Auto-install without an explicit flag (the README promise says "explicit
  install path"; keep it opt-in by design).

## Git workflow

- Single commit: `docs(plans): record editor install path design`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: PoC the mechanics

Build the VSIX (`mise run vscode-package`), then against whichever editor
CLIs exist on this machine (`code`, `cursor` — probe both): install from the
local VSIX, verify with `--list-extensions`, uninstall
(`--uninstall-extension criv.vscode-criv`). Record every command, its exact
output, and exit code — including forcing the failure cases (nonexistent
VSIX path; a CLI not on PATH). These transcripts anchor the design's error
handling.

### Step 2: Settle the design questions

Write the report answering, with a recommendation each:

1. **Command surface**: `criv init --install-vscode[=editor]` vs a separate
   `criv install-editor <editor>` subcommand. Consider: init is also re-run
   on existing vaults; a separate subcommand avoids overloading init and
   matches "explicit install path". Recommend one.
2. **Artifact source**: local VSIX (works today, but a TARGET repo running
   `criv init` has no VSIX — would criv download one from GitHub releases?
   That adds network + integrity questions) vs published-ID install (clean,
   but blocked on marketplace/Open VSX publication). Likely answer: the
   feature is gated on publication; scope "publish the extension" as the
   prerequisite work item, and support `--vsix <path>` as the
   developer/offline override. Argue it either way with the PoC data.
3. **Editor detection**: explicit editor argument vs auto-probe of
   known CLIs (`code`, `cursor`, others from ADR-0035's derivative list).
   Follow the `run_optional_tool` precedent: explicit selection, clear skip
   message when the CLI is missing, never a hard failure of `init` itself.
4. **Failure UX**: exact messages for no-CLI, install-command-failure
   (nonzero exit → surface stderr), already-installed (idempotency —
   `--install-extension` on an installed extension: record what the PoC
   showed).
5. **Testability**: how the eventual feature gets tested without an editor
   in CI (a fake `code` script on PATH in a temp dir — the init test suite
   `src/init/tests.rs` pattern supports this).

### Step 3: Outline the follow-up plan

End the report with the build-plan outline: flag/subcommand shape, the
`src/init.rs` (or new module) touch points, the test matrix, and the
publication prerequisite with its own decision owner. Include the README
lines to update when the promise is delivered.

**Verify**: report committed with PoC transcripts, five answered questions,
one recommendation each, and the follow-up outline; `git status` shows only
the report (plus `plans/README.md`); the VSIX and any installed test
extension are cleaned up (`--uninstall-extension` run; `vscode-criv.vsix`
is gitignored — confirm with `git status`).

## Test plan

Not applicable (spike). The PoC transcripts are the evidence.

## Done criteria

- [ ] `plans/reports/016-editor-install-spike.md` committed with PoC
      transcripts (≥1 real editor CLI exercised, or evidence none exists on
      this machine and STOP was reported), the five design answers, and the
      follow-up outline
- [ ] No production code changed; no extension left installed from the PoC
- [ ] `plans/README.md` status row updated

## STOP conditions

- No editor CLI (`code`, `cursor`, or compatible) exists on this machine and
  none can be reasonably obtained — write up what's answerable from docs
  alone, mark the PoC section blocked, and report.
- `mise run vscode-package` fails (vsce missing/broken) — report; don't
  install tooling globally to work around it.

## Maintenance notes

- The design's publication prerequisite (marketplace / Open VSX) is a
  human decision with account ownership implications — flag it clearly; no
  agent should create publisher accounts.
- When the feature ships, `README.md:99-103` and ADR-0035 context should be
  updated in the same PR, and `criv init`'s help text becomes the canonical
  doc for the flag.

# Agent Guide

This file is for agents developing criv itself. criv is a Rust CLI under
`src/` that validates a `docs/` vault, with TypeScript companions in
`.obsidian/plugins/criv` and `extensions/vscode-criv`, plus the
`crates/criv-wasm` helper. This repository is also a criv vault, so docs and
ADR changes are checked by criv. The runtime skills installed by `criv init`
are for agents using criv in another repository; this file is for work on this
repository.

## Verification

| Command | When to use |
| --- | --- |
| `mise run check` | Run before finishing any change; it is what CI runs. |
| `cargo test --workspace` | Rust CLI, wasm crate, and integration tests. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lint gate. |
| `cargo run --quiet -- check` | Fast criv vault validation after docs/code edits. |
| `npm --prefix .obsidian/plugins/criv test` | Obsidian companion plugin tests. |
| `npm --prefix extensions/vscode-criv test` | VS Code companion extension tests. |
| `mise run fix` | Apply configured formatting/fixers to modified files. |

## Conventions

- Use conventional commits, for example `perf(search): limit unfiltered file search candidates`.
- `.criv/` is generated local state and stays ignored by git.
- Files under `docs/` are vault notes; include frontmatter and keep
  `criv check` passing.
- Accepted ADRs are immutable per ADR-0012; add a new ADR for new decisions.
- Open findings live in GitHub Issues; settled decisions belong in `docs/adr/`.
  Do not add a findings file to the repository — there is one tracker.

## Agent skills

### Issue tracker

Agent skills file and read tickets in GitHub Issues on `TudorAndrei/criv` via the
`gh` CLI. It is the only tracker. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical roles map to identically-named GitHub labels; only `wontfix`
exists so far. Closing an issue as done or `wontfix` requires an ADR when it
settles how criv behaves. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `docs/adr/` is the primary domain record, with no `CONTEXT.md`
yet. See `docs/agents/domain.md`.

## Pointers

- `docs/tooling.md` covers mise, hk, hooks, and local task details.
- `docs/releasing.md` covers release workflow and checks.
- `docs/adr/README.md` is the decision index.

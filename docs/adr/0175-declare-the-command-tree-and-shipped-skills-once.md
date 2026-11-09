---
id: ADR-0175
kind: decision
title: Declare the Command Tree and Shipped Skills Once
status: accepted
date: 2026-11-06
governs:
  - src/lib.rs
  - src/usage_json.rs
  - src/init.rs
  - src/init/templates.rs
  - src/install.rs
  - src/install/skills.rs
  - src/install/editor.rs
---

# Declare the Command Tree and Shipped Skills Once

## Context

[[0135-answer-agents-with-codes-fixes-and-next-commands|ADR-0135]] adds the
hidden `--usage-json` flag. Its serialization model was in `src/lib.rs`, beside
command dispatch, and the tests repeated the fourteen query subcommand names.

The shipped skills had three lists: one `include_str!` constant for each skill,
an agent path, and a Claude path for each skill. A new folder under
`assets/skills/` that was not added by hand was not installed, and no test
showed the gap. `src/install/skills.rs` also had test-only wrappers and a
`linker` closure parameter that only one test used.
[[0128-enforce-runtime-paths-through-repository-files|ADR-0128]] keeps
`src/install.rs` as the only installation interface. That file renamed
`install_from` to `install_skills_from` and `run` to `install_editor`.

## Decision

`src/usage_json.rs` owns the `--usage-json` model and writes it from the
`usage-rs` command tree. `src/lib.rs` parses and dispatches only. Tests read
the query subcommand names from the declared command tree.

`src/install/skills.rs` declares each shipped skill once, by its folder name
under `assets/skills/`. The agent path and the Claude path come from that
name. A test compares the table with the folders in `assets/skills/`.

`src/install.rs` re-exports `install_skills` and `install_editor` with their
own names. The child modules have no test-only functions. Tests use the
production functions through a `RepositoryFiles` handle.

`criv init` creates `docs/adr` and the snapshot directory before it writes
`criv.toml`, the empty State, and the ADR index, and installs skills through
one code path for create-only and refresh modes.

## Consequences

A new skill needs one table line, and the test fails until the line exists.
The `--usage-json` output and the `criv init` output do not change. When
`criv init` refuses a symlinked `docs/` or `.criv/`, it no longer writes
`criv.toml` first.

The copy fallback for platforms without directory links is tested directly,
because no supported test platform reports that links are unsupported.

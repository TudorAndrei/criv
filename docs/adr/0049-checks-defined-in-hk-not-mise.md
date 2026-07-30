---
id: ADR-0049
kind: decision
title: Checks Defined In hk Not Mise
status: accepted
date: 2026-07-30
supersedes:
  - ADR-0013
governs:
  - hk.pkl
  - mise.toml
---

# Checks Defined In hk Not Mise

## Context

[[0013-mise-managed-hk-hook-toolchain|ADR-0013]] split responsibilities cleanly:
mise is the tool installer and task front door, hk is the hook and check
orchestrator, and "`hk.pkl` owns the hook behavior".

The implementation drifted from that last clause. Nine hk steps did not own their
behavior at all — they shelled out to a mise task that held the real command:

```pkl
local plugin_lint = new Step {
  glob = plugin_files
  check = "mise run plugin-lint"     // real command lives in mise.toml
}
```

That indirection cost more than it bought.

**Ownership was split across two files.** Reading what the `vscode-test` step
actually runs meant opening `hk.pkl`, finding the step, then opening `mise.toml`
to find the task. Changing a command meant editing the file that does not
describe the check.

**The trigger globs were wrong in a subtle way.** Because the commands lived in
`mise.toml`, every affected step listed `mise.toml` in its `glob` so that editing
a command re-ran the step. That made unrelated edits — a tool version bump, a new
release task — re-run the Obsidian plugin suite and the VS Code integration host.

**A process was spawned to spawn a process.** Each step paid a `mise run`
invocation, and mise task resolution, before reaching `npm` or `cargo`.

**The tasks were not a real entry point.** `mise run vscode-test` and
`mise run vscode-package` had zero references anywhere in the repository or its
documentation. hk already offers `hk check --all --step <name>`, which is the
supported way to run one step and does not require a parallel task to exist.

## Decision

Every check, test, lint, and format step hk runs defines its command inline in
`hk.pkl`. No hk step shells out to `mise run`.

The rule is: **if hk runs it, hk defines it.**

The nine tasks whose only caller was an hk step are removed from `mise.toml`:
`hawk`, `plugin-build`, `plugin-lint`, `plugin-format-check`, `plugin-test`,
`vscode-lint`, `vscode-format-check`, `vscode-test`, and
`vscode-json-diagnostics`.

mise keeps what is genuinely its own:

- **Tool installation and pinning** — `[tools]`, `mise.lock`, and the
  `hk install --mise` postinstall hook. This is unchanged and is the larger half
  of ADR-0013.
- **Hook entry points** — `check`, `fix`, `pre-commit`, `pre-push`, `commit-msg`.
  These invoke hk; they do not define checks.
- **Non-check tasks** — `perf`, `release-plan`, `release-auto`, `vscode-build`,
  `vscode-package`.

Steps that hk runs now list `hk.pkl` in their `glob` alongside `mise.toml`, since
both genuinely affect them: `hk.pkl` holds the command, and `mise.toml` still
pins the Node and Rust versions the command runs under.

This supersedes ADR-0013. The division of labour it established is kept and
sharpened; what changes is that "`hk.pkl` owns the hook behavior" is now literal
rather than aspirational, and its Consequences section's "hook-specific mise
tasks" no longer includes per-check tasks.

## Consequences

A step's command is visible where the step is defined. `hk check --all --plan`
lists every step, and `hk check --all --step <name>` runs one.

Editing `mise.toml` no longer re-runs the JavaScript and TypeScript suites for
reasons unrelated to them.

Two earlier decisions named their checks by mise task. Both decisions stand
unchanged; only the invocation path moves.
[[0037-vscode-json-diagnostics-in-hooks|ADR-0037]] exposed the integration check
as `mise run vscode-json-diagnostics`; it is now the `vscode-json-diagnostics` hk
step. [[0043-hawk-visibility-analysis|ADR-0043]] ran Hawk through `mise run
hawk`; it is now the `hawk` hk step, with the same flags and the same
`target/hawk` isolation.

Because hk hooks are installed with `hk install --mise`, steps still execute
inside the mise environment and resolve the pinned toolchain. Removing the task
layer does not remove tool pinning.

The trade accepted here is that a contributor can no longer discover these checks
through `mise tasks`. `hk check --all --plan` is the replacement, and
`docs/tooling.md` points at it.

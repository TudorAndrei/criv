---
name: checking-drift
description: Use when a criv command fails or prints a diagnostic, when reading a criv error code, fix line, or next command, or when validating that documents, ADR metadata, wiki-links, source references, and generated state still match the code.
metadata:
  criv-template: blake3:a1a8161ab266a003
---

# Checking drift

Drift is the gap between what a document claims and what the code does. `criv
check` is the whole verdict: the vault is **green** when the command prints
`criv check: ok`. Green is the completion criterion for every criv workflow.

## Getting to green

1. Run `criv watch --once` to refresh `.criv/state.json`. A stale state
   produces diagnostics that describe the previous code.
2. Run `criv check` and read every diagnostic. Each one carries a `fix:` line,
   and the run ends with one `next:` command. Use `criv check --filter <text>`
   to narrow the output while investigating one failure, and `criv check
   --format json` when an agent or a script consumes the diagnostics.
3. Run the command the `next:` line names. It is `criv check --fix` when criv
   can repair the vault itself.
4. Run `criv enforce --stage ci` when the change touches ADR-governed source.

Repeat until a run that included your change comes back green.

## Reading a diagnostic

A diagnostic names a severity, a code, a file, and a message. Most carry a
`fix:` line under them:

```text
error[unresolved-governs] docs/adr/0007-example.md: governs glob `src/old.rs` matches no source files
  fix: Run `criv adr reconcile-sources --base <ref>` for a rename, or add a successor ADR for a deletion.
next: apply the fixes above, then run `criv check`
```

Run the `fix:` line rather than inventing a command. The `next:` line at the end
names the one command to run after the repair. `criv check --format json`
carries the same `code` and `fix` as fields.

Correct the file the diagnostic names, or correct the code that made the claim
false. Treat a diagnostic that misreads the repository as a criv bug worth
reporting.

A governs or policy diagnostic names an ADR. Run `criv check --filter ADR-NNNN`
to isolate its current diagnostics. The `writing-decisions` skill owns the ADR
side of that fix.

## Reading a failure

A failure that is not a diagnostic prints its code in brackets first, and often
a `fix:` line:

```text
criv: [not-a-vault] not a criv vault: no criv.toml in /work/other-repo
fix: criv init
```

Key recovery on the code, never on the prose. These codes matter most:

| Code | Meaning |
| --- | --- |
| `not-a-vault` | The working directory holds no `criv.toml`. Check the directory before running `criv init`. |
| `check-failed` | `criv check` found errors. The diagnostics above it say which. |
| `policy-violation` | An ADR policy pattern matched. Change the code, or supersede the ADR. |
| `import-policy-violation` | An import broke a rule in `criv.toml`. |
| `adr-immutability-violation` | An accepted ADR changed. Restore it and write a successor. |

Exit codes are the fast signal: `0` succeeded, `1` the command failed, `2` the
command line was wrong.

## Machine-readable runs

- `criv check --format json` — diagnostics with `code`, `path`, `line`, `message`, and `fix`.
- `criv enforce --stage ci --format json` — one object with the counts, the violations, the failure `code`, and its `fix`.
- `criv watch --once --format json` — the snapshot, the counts, and the next command.
- `criv --usage-json` — the whole command tree, with every flag, choice, and default.

Never guess a flag. Read it from `criv <command> --help` or from
`criv --usage-json`.

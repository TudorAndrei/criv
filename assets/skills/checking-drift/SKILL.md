---
name: checking-drift
description: Use when criv check fails, or when validating that documents, ADR metadata, wiki-links, source references, and generated state still match the code.
---

# Checking drift

Drift is the gap between what a document claims and what the code does. `criv
check` is the whole verdict: the vault is **green** when the command prints
`criv check: ok`. Green is the completion criterion for every criv workflow.

## Getting to green

1. Run `criv watch --once` to refresh `.criv/state.json`. A stale state
   produces diagnostics that describe the previous code.
2. Run `criv check` and read every diagnostic. Use `criv check --filter <text>`
   to narrow the output while investigating one failure, and `criv check
   --format json` when an agent or a script consumes the diagnostics.
3. Run `criv check --fix` to apply the safe Markdown fixes, then check again.
4. Run `criv enforce --stage ci` when the change touches ADR-governed source.

Repeat until a run that included your change comes back green.

## Reading a diagnostic

Each diagnostic names a file and a rule. Correct the file it names, or correct
the code that made the claim false. Treat a diagnostic that misreads the
repository as a criv bug worth reporting.

A governs or policy diagnostic names an ADR. Run `criv check --filter ADR-NNNN`
to isolate its current diagnostics. The `writing-decisions` skill owns the ADR
side of that fix.

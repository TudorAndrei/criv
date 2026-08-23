---
id: ADR-0135
kind: decision
title: Answer Agents with Codes Fixes and Next Commands
status: accepted
date: 2026-08-23
governs:
  - src/check.rs
  - src/enforce.rs
  - src/lib.rs
  - src/query.rs
  - src/repository.rs
  - src/watch.rs
---

# Answer Agents with Codes Fixes and Next Commands

## Context

[[0134-parse-the-cli-with-usage|ADR-0134]] made `--help` the source of truth for
criv documentation, because criv runs in other people's repositories where an
agent has the installed binary and nothing else.

Help alone is not enough. An agent cannot see the screen. It decides what
happened from the exit code, and it decides what to do next from the text it
gets back. Three published treatments of agent-facing command-line design agree
on the same practices, and one of them is the practice that separates a CLI an
agent can drive from a human CLI with `--json` added:

- Every response carries the exact command to run next.
- Every failure carries a stable identifier and the repair.
- The tool describes itself at runtime, so an agent needs no external document.
- Output can be limited before it is produced, because an agent pays for every
  field it reads.

criv answered none of these completely. Diagnostics carried stable codes but no
repair. Errors carried neither. `criv enforce` and `criv watch --once` spoke
only prose, so an agent had to match the words "enforcement passed". Queries
returned the whole graph.

Worse, criv reported success where no vault exists. In an empty directory,
`criv check` printed `criv check: ok` and exited 0. Under criv's own contract
that line is the completion criterion for every workflow, so a directory
mistake, or a repository where nobody ran `criv init`, read as a green vault.

## Decision

Fail closed when there is no vault. `check`, `query`, `watch`, `enforce`, and
`adr` require `criv.toml` under the working root. Without it they exit 1 with
the code `not-a-vault` and the repair `criv init`. `init` and `install-editor`
stay usable outside a vault, because they are how a vault begins.

Give every diagnostic a repair. `fix_for` in `src/check.rs` maps each
diagnostic code to the command or the edit that clears it. The text report
prints it under the diagnostic as `fix:`, and the JSON report carries it as a
`fix` field. A code with no mechanical repair carries none, rather than carrying
a guess.

Name the next command. A failed `criv check` ends with a `next:` line that
names one command: `criv check --fix` when criv can repair the vault itself,
`criv watch --once` when the state is stale, and otherwise the command that
proves a hand edit worked. `criv watch --once` ends with `next: criv check`.

Give failures stable identifiers. `CrivError::Coded` carries a code, a message,
and an optional repair, and it renders as `[code] message` with a `fix:` line.
Enforcement failures use `policy-violation`, `import-policy-violation`,
`adr-immutability-violation`, and `enforcement-failed`. A check that found
errors uses `check-failed`.

Report enforcement and refresh as data. `criv enforce --format json` prints one
object with the stage, the counts, the comparison basis, the violations, the
failure code, and the repair. `criv watch --once --format json` prints the
snapshot, the counts, and the next command. Both keep their text reports as the
default, and both keep their exit codes.

Let a caller bound a query before it runs. Every query accepts `--limit <N>`,
and `--format ndjson` prints one JSON row per line for incremental reading.

Describe the command tree as JSON. The hidden `--usage-json` flag prints the
tree with each command's path, help, arguments, flags, choices, defaults,
conflicts, and requirements. It is built from the parse tables criv already
declares, so it needs no additional dependency, and it complements the KDL that
`--usage` prints for the `usage` CLI.

## Consequences

`criv check` now fails in a directory that is not a vault, where it printed
`criv check: ok` before. This is the point of the change: a false green is worse
than a failure. A caller that relied on criv succeeding outside a vault must run
`criv init` first.

The text report grows a `fix:` line under each diagnostic and one `next:` line
at the end. A caller that matches whole lines is unaffected; a caller that
counts output lines is not.

Enforcement error messages now begin with `[code]`. The prose after the code is
unchanged.

The JSON diagnostic gains an optional `fix` field. Existing fields keep their
names and meanings.

`criv --usage` keeps its KDL output for the `usage` CLI, unchanged.

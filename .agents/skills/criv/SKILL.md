---
name: criv
description: Use when working in a criv vault, refreshing criv state, looking for undocumented code, or choosing which criv skill covers a task.
metadata:
  criv-template: blake3:f6afc69a8f9f55b4
---

# criv

criv checks the configured ADR folder and enforces its source policies.
General documentation outside that folder belongs to its documentation tool.
A vault is
**green** when `criv check` prints `criv check: ok`. Every criv workflow ends
green.

## The loop

- `criv watch --once` refreshes `.criv/state.json` after a source or ADR change.
- `criv check` reports drift.
- `criv enforce --stage ci` gates a change to ADR-governed source.

The `checking-drift` skill owns the flags and the failure paths.

## Finding what is undocumented

- `criv query nodes --kind code --without-docs` lists code that no ADR or decision index claims.
- `criv query coverage --by module` and `criv query coverage --by adr` report coverage.
- `criv query --help` holds the full query surface. Add `--limit <N>` to bound a
  large answer, and `--format ndjson` to read one row per line.

## Reading a failure

A `criv check` diagnostic always names a code and a repair. Other failures name
a code only when criv has a defined recovery for them; the rest report prose.
When a `[code]` or a `fix:` line is present, use it and run the command it
names. The `checking-drift` skill holds the codes, the machine-readable
formats, and the recovery paths.

Never guess a flag: read it from `criv <command> --help`, or from
`criv --usage-json` for the whole command tree.

## Which skill covers the task

- `referencing-code` — wiki-link syntax from a document or ADR to source.
- `writing-decisions` — creating an ADR with `criv adr new`, frontmatter, governs scopes, and policy patterns.
- `checking-drift` — the check loop, its flags, and how to read a diagnostic.
- `criv-me` — developing a decision with the user before writing it down.

The work is complete when the change is written, `criv watch --once` has run
after it, and the vault is green.

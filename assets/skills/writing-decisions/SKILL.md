---
name: writing-decisions
description: Use when creating or superseding a criv ADR under docs/adr, setting its governs scopes, or adding an inline policy pattern.
---

# Writing decisions

A decision note lives under `docs/adr/` as `YYYYMMDD-slug.md` and carries
`id`, `kind: decision`, `title`, `status`, and `date`. Use one H1 matching
`title`, then H2 `Context`, `Decision`, and `Consequences` sections, once each
and in that order. Active ADRs and drafts must meet this format; accepted
historical records keep their original bytes when superseded.

Start every new decision with `criv adr new "<title>"`. It writes the draft
`docs/adr/YYYYMMDD-slug.md` and prints its path. The ID is
`ADR-YYYYMMDD-slug`: the UTC date of today and the slug of the title. The file
name is the ID without `ADR-`, and the `id` field is the ID. The draft has
`status: draft`, the date, the H1, and empty `Context`, `Decision`, and
`Consequences` sections. Fill in the three sections, add `governs:`, and set
`status` when the decision is ready. The command fails, and writes nothing,
when that ID already exists; choose a more specific title. Older ADRs keep
their `ADR-NNNN` IDs and `NNNN-slug.md` names, but `criv enforce` rejects a new
file with a `NNNN-` name as `legacy-adr-id`. Never write the ID or the file
name by hand.

## Governed scope

`governs:` lists the path globs and source selectors the decision controls.
Each glob must match at least one indexed source file; `criv check` reports an
unmatched glob as `unresolved-governs`. Name the files a reader would open, and
let the decision govern only what it really decides.

## Policy patterns

State an enforceable structural rule as an inline `policy.patterns` entry, so
the rule and its rationale stay in one file:

```yaml
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Prefer structured diagnostics.
```

Use `pattern` for a simple ast-grep pattern and `rule` for full ast-grep YAML.
An ADR's `policy.patterns` frontmatter is the only home for a persistent named
rule, addressed as `<ADR-ID>/local-id`, for example
`ADR-20261117-cache-compiled-policies/no-println`.

Inspect policy diagnostics before accepting the decision:

- `criv check --filter <ADR-ID>` — text diagnostics for one decision, each with its `fix:` line.
- `criv check --format json --filter <ADR-ID>` — the same diagnostics with `code` and `fix` as fields.
- `criv enforce --stage ci` — the complete policy gate. Add `--format json` for
  the counts, the violations, and the failure code.

## Immutability

An accepted ADR is a historical record. Change a decision by writing a new ADR
whose `supersedes:` names the old one, and leave the old file exactly as it is.

The decision is complete when its globs resolve, its patterns match what the
message describes, and the vault is green.

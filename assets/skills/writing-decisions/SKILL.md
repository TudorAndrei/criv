---
name: writing-decisions
description: Use when creating or superseding a criv ADR under docs/adr, setting its governs scopes, or adding an inline policy pattern.
---

# Writing decisions

A decision note lives under `docs/adr/` as `NNNN-kebab-title.md` and carries
`id`, `kind: decision`, `title`, `status`, and `date`. Take the ID from `criv
query next-adr-id`, which reads the state and returns the next free number.

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
rule, addressed as `ADR-NNNN/local-id`.

Inspect matches before accepting the decision:

- `criv search --pattern-id ADR-NNNN/local-id` — one named rule, scoped to its ADR's `governs`.
- `criv search --rule ADR-NNNN` — every rule in that ADR.
- `criv search --lang rust 'pattern'` — an exploratory pattern with no ADR.

## Immutability

An accepted ADR is a historical record. Change a decision by writing a new ADR
whose `supersedes:` names the old one, and leave the old file exactly as it is.

The decision is complete when its globs resolve, its patterns match what the
message describes, and the vault is green.

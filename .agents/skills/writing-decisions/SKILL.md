---
name: writing-decisions
description: Use when creating or updating criv ADRs under docs/adr with required metadata, governs scopes, and policy patterns.
metadata:
  criv-template: blake3:ac97284e11e24fd3
---

# Writing decisions

Decision notes use `kind: decision`, an ID like `ADR-0001`, and live under `docs/adr/`.
Get the next available ID with `criv query next-adr-id`; do not guess it.

Required fields:

- `id`
- `kind: decision`
- `title`
- `status`
- `date`

Use `governs:` to list path globs or source selectors controlled by the decision.

Use `policy.patterns:` for ast-grep rules that enforcement should evaluate. Prefer inline policy definitions when the ADR states a structural rule:

```yaml
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Prefer structured diagnostics.
```

Use `pattern` for simple ast-grep patterns and `rule` for full ast-grep YAML. Test one named rule with `criv search --pattern-id ADR-NNNN/local-id`, every rule in an ADR with `criv search --rule ADR-NNNN`, or an unnamed exploratory pattern with `criv search --lang rust 'pattern'`.

Persistent named structural rules belong only in ADR `policy.patterns` frontmatter. Use the full `ADR-NNNN/local-id` identifier.

Accepted ADRs are immutable. Do not edit, delete, or rename an existing ADR to change a decision. Create a new ADR and use `supersedes:` to point to the older decision when the new decision replaces it.

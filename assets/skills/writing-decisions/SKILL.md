---
name: writing-decisions
description: Use when creating or updating criv ADRs under docs/adr with required metadata, governs scopes, and policy patterns.
---

# Writing decisions

Decision notes use `kind: decision`, an ID like `ADR-0001`, and live under `docs/adr/`.

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

Use `pattern` for simple ast-grep patterns and `rule` for full ast-grep YAML. Test non-trivial rules with `criv search --rule ADR-NNNN` or `criv check --filter policy` before finishing.

Do not put ADR-owned structural rules in `criv.toml` as `[patterns."ADR-NNNN/..."]`. ADR-owned rules belong in the ADR's `policy.patterns` frontmatter.

Accepted ADRs are immutable. Do not edit, delete, or rename an existing ADR to change a decision. Create a new ADR and use `supersedes:` to point to the older decision when the new decision replaces it.

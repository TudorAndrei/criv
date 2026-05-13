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

Use `governs:` to list path globs controlled by the decision. Use `policy.patterns:` for ast-grep rules that enforcement should evaluate.

Accepted ADRs are immutable. Do not edit, delete, or rename an existing ADR to change a decision. Create a new ADR and use `supersedes:` to point to the older decision when the new decision replaces it.

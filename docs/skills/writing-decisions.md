---
id: writing-decisions
kind: doc
title: Writing decisions
tags: [criv, skill]
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

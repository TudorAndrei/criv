---
name: criv
description: Use when working in a criv vault to keep docs, ADRs, source references, checks, state, and enforcement in sync with code changes.
---

# criv

Use `criv` to keep repository documentation connected to source code.

Core workflow:

- Run `criv watch --once` after code or docs changes to refresh `.criv/state.json`.
- Run `criv check` before declaring documentation work complete.
- Use `criv query nodes --kind code --without-docs` to find undocumented code.
- Use `criv query coverage --by module` and `criv query coverage --by adr` to inspect documentation coverage.
- Use `criv enforce --stage ci` before finishing changes that affect ADR-governed code.

Write docs and ADRs with wiki-links to source paths, symbols, patterns, and notes.

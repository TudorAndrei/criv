---
name: checking-drift
description: Use when validating whether criv documentation, ADR metadata, wiki-links, source references, and generated state still match the code.
---

# Checking drift

Run `criv check` after editing documentation.

Run `criv watch --once` after changing code or docs to refresh `.criv/state.json`.

Use `criv check --format json` when an agent or script needs machine-readable diagnostics.

For ADRs with inline `policy.patterns`, run `criv search --rule ADR-NNNN` to inspect matches and `criv enforce --stage ci` before finishing changes that affect governed source.

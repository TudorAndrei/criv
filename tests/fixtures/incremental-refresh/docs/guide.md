---
id: fixture-guide
kind: doc
title: Incremental Refresh Fixture
targets:
  symbols:
    - src/lib.rs#fn:run
---

# Incremental Refresh Fixture

The behavior is governed by [[0001-no-println|ADR-0001]]. The Rust entry point
is `src/lib.rs#fn:run`.

## Operation

The fixture exercises stable headings, links, and source references.

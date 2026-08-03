# Performance Harness Internals

This directory contains deterministic workload generation, sample isolation,
and result summarization used by `scripts/measure-performance.sh`. The public
measurement contract lives in `docs/performance.md` and
[[0069-repeatable-two-tier-performance-evidence|ADR-0069]].

Canonical workload inputs live in `fixtures/performance/`. Generated vaults and
measurement results are temporary or user-selected output and are never
committed.

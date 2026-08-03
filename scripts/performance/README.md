# Performance Harness Internals

This directory contains deterministic workload generation, sample isolation,
and result summarization used by `scripts/measure-performance.sh`. The public
measurement contract lives in `docs/performance.md` and
[[0069-repeatable-two-tier-performance-evidence|ADR-0069]].

Canonical workload inputs live in `fixtures/performance/`. Generated vaults and
measurement results are temporary or user-selected output and are never
committed.

`render-git-note.sh` validates a completed release-profile result directory and
reduces it to the deterministic JSON summary stored by the push workflow.
`publish-git-note.sh` replaces the pushed commit's note on
`refs/notes/criv-performance`, refreshing and retrying when another workflow
updates the notes ref concurrently. It never force-pushes the remote ref.

The publication path has two local smoke tests:

```sh
tests/performance_harness.sh
tests/performance_git_note.sh
```

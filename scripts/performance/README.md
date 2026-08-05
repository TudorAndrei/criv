# Performance Harness Internals

This directory contains deterministic workload generation, sample isolation,
and result summarization used by `scripts/measure-performance.sh`. The public
measurement contract lives in `docs/performance.md` and
[[0072-keep-performance-observation-outside-core|ADR-0072]].

Canonical workload inputs live in `fixtures/performance/`. Generated vaults and
measurement results are temporary or user-selected output and are never
committed.

`render-git-note.sh` validates a completed release-profile result directory and
reduces it to the deterministic JSON summary stored by the push workflow.
`criv-perf-report` validates the same evidence and renders a dependency-free
HTML report with shared-scale timing plots, exact-value tables, provenance, and
a compact GitHub job summary. The report is derived presentation; JSON remains
the canonical evidence.
`publish-git-note.sh` replaces the pushed commit's note on
`refs/notes/criv-performance`, refreshing and retrying when another workflow
updates the notes ref concurrently. It never force-pushes the remote ref.

The publication path has two local smoke tests:

```sh
tests/performance_harness.sh
tests/performance_git_note.sh
```

## State storage baseline

`criv-state-storage-baseline` reads one generated `.criv/state.json`, reports
its graph and repeated-string shape, and records repeated native
read/decode/schema-validation samples. It measures the current JSON boundary;
it does not call private `criv` functions.

`measure-state-wasm.mjs` measures the packaged Wasm boundary in fresh Node.js
processes. It records cold module plus initial-projection cost, initial
projections after load, graph lookup, selector variants, Wasm module bytes, and
process maximum RSS.

Use release artifacts and at least three samples for evidence:

```sh
cargo run --release -p criv-perf-harness \
  --bin criv-state-storage-baseline -- \
  --state /path/to/generated/.criv/state.json \
  --samples 5

node scripts/performance/measure-state-wasm.mjs \
  --state /path/to/generated/.criv/state.json \
  --package extensions/vscode-criv/pkg \
  --samples 5
```

The main harness also has `state-list` and `state-prune-dry-run` cases. Their
untimed setup publishes twenty unique snapshots before it measures the public
CLI command. This matches the default retained snapshot bound.

## State store candidate prototype

`state-store-prototype/` contains the throwaway candidate adapters for GitHub
issue 88. `criv-state-storage-fixtures` generates the matched State revisions
from the canonical observed workload manifests. The candidate CLI and its
packaged Wasm adapter then measure storage, publication, native operations, and
editor projections through public process boundaries. See
`state-store-prototype/README.md` for the exact commands.

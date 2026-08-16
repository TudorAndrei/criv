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

`measure-state-wasm.mjs` measures the packaged Wasm loaded-revision boundary in
fresh Node.js processes. It records cold module plus State load and initial-batch
cost, the initial batch after revision load, prepared graph lookup, prepared
selector variants, twenty replace-and-free cycles, Wasm module bytes, and
process maximum RSS. Each timed query uses one loaded revision and does not pass
raw State again.

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

## State store candidate prototype

`state-store-prototype/` contains the throwaway candidate adapters for GitHub
issue 88. `criv-state-storage-fixtures` generates the matched State revisions
from the canonical observed workload manifests. The candidate CLI and its
packaged Wasm adapter then measure storage, publication, native operations, and
editor projections through public process boundaries. See
`state-store-prototype/README.md` for the exact commands.

## File discovery baseline

File-discovery evidence has seven separate tools:

- `criv-discovery-inventory` creates a local content-addressed inventory and a
  sanitized workload summary. The full inventory contains paths and stays
  outside source control. The summary includes hidden and ignored shape,
  configured profile roots and patterns, and per-top-level-subtree entry and
  byte counts. These summary fields do not change the content-addressed
  workload digest.
- `criv-discovery-snapshot` creates one strict APFS copy-on-write snapshot. It
  never falls back to a full copy and refuses to run below its free-space gate.
- `criv-discovery-baseline` runs the release-profile, test-only Source, Vault,
  and Markdown selector probes. It records raw samples, elapsed and CPU time,
  peak resident memory, selected counts and bytes, and stable path digests.
- `criv-discovery-fixtures` generates the deterministic scaling trees.
- `criv-discovery-edge-fixtures` generates focused correctness repositories
  for parity, profile rules, selected links, invalid patterns, missing roots,
  and non-UTF-8 path identity.
- `criv-discovery-adapter` exports an immutable criv revision and applies only
  its fixed test-only probe adapter.
- `criv-discovery-commands` measures official release commands on strict
  workload snapshots. It includes cold and warm one-shot publication, live
  readiness and convergence, full check, and changed Source and Markdown
  checks.

Build the probe and run a smoke measurement with:

```sh
cargo run --locked -p criv-perf-harness \
  --bin criv-discovery-baseline -- \
  --workload-root /path/to/workload \
  --workload-inventory /path/to/local-inventory.json \
  --samples 1 \
  --allow-low-samples
```

The probe is compiled only in the root library test executable. It calls the
real private selectors without adding a command, feature, public API, or
measurement protocol to the production binary. Full evidence uses five
samples. Low-sample smoke runs can supply `--workload-id` and
`--workload-digest` directly. A relative median absolute deviation above ten percent causes one
complete repeat and marks a second unstable attempt as unsuitable for a gate.
Use `--dump-paths` only with a one-sample low-sample run. It records complete
path lists for correctness evidence and is not timing evidence.

Official command evidence uses an explicit release artifact and strict
snapshots:

```sh
cargo run --locked --release -p criv-perf-harness \
  --bin criv-discovery-commands -- \
  --binary /path/to/official/criv \
  --snapshot-executable target/release/criv-discovery-snapshot \
  --workload-root /path/to/golden/workload \
  --workload-inventory /path/to/local-inventory.json \
  --sample-root /path/to/disposable/sample-parent \
  --source-mutation-path path/to/tracked-source \
  --markdown-mutation-path path/to/tracked-note.md \
  --live-mutation-directory path/to/source-directory \
  --samples 5
```

The command runner records failed commands as raw evidence and fails the run.
It also fails when successful sample outputs do not have one stable identity.
Published State identity and source-graph cache identity are separate because a
read-only changed check can refresh the cache without publishing a new State.

Synthetic discovery manifests live in `fixtures/performance/discovery/` and
are generated by `criv-discovery-fixtures`. They test scaling only. They are
not representative user-project evidence.

## File-discovery acceptance

`criv-discovery-gate` validates one prepared `gate-input.json`. It compares the
matched baseline and candidate command runs, the three profile scaling runs,
four target artifacts, and clean-build evidence. It writes the short-lived
`criv.discovery-release-gate.v1` receipt only when every correctness,
stability, time, memory, artifact, and toolchain gate passes.

The controlled `Discovery release gates` workflow runs on the named macOS ARM
runner. It uploads the complete evidence bundle and the four measured binaries,
then `publish-release-gate-note.sh` publishes the receipt to
`refs/notes/criv-release-gates`. Run the local receipt smoke test with:

```sh
tests/performance_release_gate_note.sh
```

The tag workflow accepts only a current receipt for the exact tagged commit. It
verifies every measured binary by SHA-256 before it packages that same binary.

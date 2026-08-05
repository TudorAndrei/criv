# State Store Candidate Prototype

This crate is throwaway benchmark code for GitHub issue 88. It does not change
the production State path. It compares these representations of the same
logical `criv.state.v1` data:

- compact JSON baseline;
- Postcard full-decode control;
- criv-owned partitioned columns;
- partitioned FlatBuffers;
- partitioned checked rkyv archives.

The three partitioned candidates use the same first-use string table, 256-row
blocks, `u32` graph endpoints, content-addressed partition pool, and small
manifests. This keeps the data layout constant while the partition codec
changes.

Build the native CLI and one packaged Wasm adapter per candidate in release
mode. Replace `json` with each candidate name:

```sh
cargo build --release -p criv-state-store-prototype
wasm-pack build scripts/performance/state-store-prototype \
  --target nodejs \
  --release \
  --out-dir target/state-store-wasm-json \
  --no-default-features \
  --features=wasm-json
```

Generate twenty State revisions from an observed workload shape:

```sh
cargo run --release -p criv-perf-harness \
  --bin criv-state-storage-fixtures -- \
  --binary target/release/criv \
  --manifest fixtures/performance/barrs-small.toml \
  --output target/state-store-fixtures/barrs-small
```

Run one candidate. Pass all generated revisions to `--snapshot`:

```sh
target/release/criv-state-store-bench \
  --candidate criv-column \
  --state target/state-store-fixtures/barrs-small/000.json \
  --changed-state target/state-store-fixtures/barrs-small/001.json \
  --snapshot target/state-store-fixtures/barrs-small/{000..019}.json \
  --wasm-package scripts/performance/state-store-prototype/target/state-store-wasm-criv-column \
  --output target/state-store-results/barrs-small-criv-column.json \
  --samples 5
```

The CLI writes machine-readable JSON to `--output`. It prints the same JSON when
that option is absent. Native load measurements use fresh child processes. Wasm
measurements use fresh Node.js processes. Publication setup is outside the
timed changed-publication interval.

Regenerate the checked-in FlatBuffers binding with FlatBuffers 25.12.19:

```sh
cd scripts/performance/state-store-prototype
flatc --rust -o src/generated schema/state_store.fbs
```

Do not move an adapter into production. The result of this prototype is input
to issue 83, which owns the final store and migration decision.

## Measurement limits

- Publication timing covers the store encode and write segment. It does not
  include State construction by `criv watch`.
- FlatBuffers and rkyv translate through the shared column form. Their timing
  includes that conservative adapter cost.
- Wasm operations keep one decoded revision for the measured operation. This
  benchmark does not settle the loaded-revision lifetime from issue 82.
- The approved workloads have no architecture payload. Architecture data is in
  correctness tests, but it has no performance result.

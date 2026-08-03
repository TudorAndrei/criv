---
id: ADR-0069
kind: decision
title: Repeatable Two Tier Performance Evidence
status: accepted
date: 2026-08-03
governs:
  - Cargo.toml
  - scripts/measure-performance.sh
  - scripts/performance/**
  - src/measurement.rs
  - mise.toml
targets:
  symbols:
    - scripts/measure-performance.sh
    - src/measurement.rs
---

# Repeatable Two Tier Performance Evidence

## Context

The existing performance script defaults to `target/debug/criv`, executes
sequential cases against one mutable vault, and reduces five `/usr/bin/time`
samples to text min/median/max rows. It does not preserve raw samples, measure
dispersion, prove cache initialization, identify the binary or workload by
content, or distinguish observed input shape from an invented scale model.

Those limitations make before/after claims fragile. A faster result can come
from the size-optimized release profile, a cache left by an earlier case, input
drift, or noise rather than the code change under review. Aggregate elapsed
time also cannot show which transformation stopped doing work.

GitHub issue #44 requested observed small, medium, and large shapes. Two public
criv vaults were available and measured at fixed revisions: `TudorAndrei/barrs`
and this repository. The maintainer approved them as small and medium. No
observed large criv vault was available, and the maintainer explicitly deferred
that tier rather than authorizing extrapolation.

Performance execution remains a non-check mise task under
[[0049-checks-defined-in-hk-not-mise|ADR-0049]].

## Decision

Adopt `fixtures/performance/barrs-small.toml` and
`fixtures/performance/criv-medium.toml` as the complete canonical workload set.
Each manifest records its observed repository and revision, note and decision
counts, indexed source files and bytes, symbol count, language/file extension
distribution, note links, source references, policies, C4 artifacts, and a
one-source mutation with its exact changed-file fraction. Workload generation
is deterministic and sanitized: it recreates those measurable distributions
without copying observed prose or source.

Do not define a synthetic large tier. Absence is explicit in documentation and
machine-readable workload enumeration. A future large tier requires an
observed sanitized manifest, maintainer review, and a new decision extending
this evidence set; medium results must not be relabeled as large.

Allow a Docker environment managed through Testcontainers for Rust as an
explicit measurement lane. The container is only an execution environment: it
does not supply, stand in for, or determine a vault shape. Container runs use
the same checked-in manifests and deterministic generator as host runs. A
future approved large manifest may use this lane without changing that data
boundary.

Pin the environment image by immutable digest and build the measured release
binary inside it. Record the image digest, Docker server identity, and
container architecture alongside the normal machine metadata. Do not copy an
incompatible host binary into a Linux container or combine host and container
rows under one machine identity. Testcontainers owns startup, file transfer or
mounts, artifact extraction, and cleanup; the harness owns the pristine vault
and cache state for each sample.

The harness requires an explicit executable binary and explicit Cargo profile.
It resolves the binary to one canonical path and records its BLAKE3 digest. It
also records repository revision and dirty state, full Rust compiler identity,
operating system/release/architecture, processor model when available, UTC
start time, harness schema, exact manifest bytes and digest, case, cache state,
and sample count. A missing, non-executable, or multiply specified binary is a
usage error. Project examples use the shipped `target/release/criv` profile;
the harness never silently selects a debug binary.

Every case declares an initial cache state. Each cold sample uses a pristine
generated vault without `.criv`. Each warm sample uses a different pristine
vault and runs only its declared untimed seed command before measurement. An
additional disposable untimed warm-up precedes the recorded samples for each
workload/case. No timed sample reuses another sample's mutable vault, cache, or
generated output.

Collect at least three samples, defaulting to five. Persist one JSONL row per
attempt with elapsed, user, and system seconds, exit status, output digests,
generated State and snapshot identities when applicable, and any structured
work record. A failed command remains in the raw data, fails the harness, and
does not enter successful timing statistics. Write a machine-readable summary
with count, minimum, median, maximum, and median absolute deviation. Always use
a new result directory so cross-commit runs cannot reuse mutable artifacts.
Comparisons are valid only between matching workload, case, cache, sample,
profile, and machine identities.

Add disabled-by-default command-local instrumentation with coarse spans and
deterministic counters for notes/source bytes, parsed/reused files, source
resolutions, policy compilations, AST parses, State construction and
serialization, cache publication, and output bytes. The harness supplies a
confined output path when it wants the record. Do not time individual nodes,
links, or symbols.

Instrumentation may observe but must not change command semantics. Tests compare
instrumented and ordinary exit status, stdout, stderr, `.criv/state.json`,
snapshot hash, and source-graph cache bytes for success and failure cases.
Deterministic work counts are the primary proof for a claim of reduced work;
repeated timing samples support that proof but do not replace it.

Keep performance execution outside hk hooks and hosted validation. Generator,
schema, isolation, failure, and summary smoke tests are normal tests; full
measurements remain an explicit `mise run perf` action because their runtime and
machine sensitivity do not define repository correctness.

Keep Docker-dependent Testcontainers tests ignored by default and expose a
separate explicit task. They require a Docker-API-compatible runtime and may
acquire the pinned image, so ordinary `cargo test --workspace`, hk, and hosted
CI do not invoke them.

## Consequences

Performance evidence becomes reproducible enough to audit and compare without
pretending one machine represents every user. Raw samples remain available when
a summary looks surprising, cache-state leakage is structurally prevented, and
binary/profile drift is visible.

The canonical set covers two observed scales and no large scale. This is less
coverage than a fabricated third workload, but every tier has real provenance.
Claims beyond medium are explicitly unsupported until an observed large vault
is approved.

The optional container lane improves toolchain and filesystem reproducibility
for expensive runs without changing workload provenance. It adds Docker image
build and acquisition cost, so those tests remain deliberately explicit.

The harness performs more setup and consumes more temporary storage because
isolation is per sample. Instrumentation also adds maintenance at core work
sites, but it stays dormant in normal commands and its parity tests make that
cost observable.

---
id: ADR-0157
kind: decision
title: Run Rust tests with cargo-nextest
status: accepted
date: 2026-11-05
governs:
  - hk.pkl
  - mise.toml
  - package.json
  - .github/workflows/ci.yml
  - tests/acceptance.rs
---

# Run Rust Tests with cargo-nextest

## Context

The hk test step, the hosted CI jobs, the editor-contract script, and the
container performance task ran Rust tests with `cargo test`. `cargo test` runs
one test binary at a time. A failed binary hides the results of the binaries
that follow it.

The Cucumber acceptance suite in `tests/acceptance.rs` uses `harness = false`.
cargo-nextest runs such a binary only when the binary supports the nextest list
and run protocol.

## Decision

Run Rust tests with `cargo nextest run`. Pin `cargo-nextest` in `mise.toml`
through the aqua backend. CI jobs that install a named tool set include
`aqua:nextest-rs/nextest/cargo-nextest`.

The acceptance binary reports the complete Cucumber suite as one nextest test
named `acceptance`. With `--list`, it prints `acceptance: test`. With `--list`
and `--ignored`, it prints nothing. When the call contains `--exact`, the binary
removes the test name, `--exact`, and `--nocapture`, and gives the remaining
arguments to the Cucumber CLI.

To run one scenario, use `cargo test --test acceptance -- --name <scenario>`.
Nextest does not send harness options to a test binary.

## Consequences

Nextest runs every test in its own process and runs test binaries in parallel.
The workspace suite passes in this mode.

Nextest does not run doctests. The workspace has no doctests. A new doctest
needs a `cargo test --doc` step in the hk test step.

Developers need `mise install` to get `cargo-nextest`. Plain `cargo test`
continues to work.

## Alternatives Considered

### One nextest test for each Cucumber scenario

Rejected. The binary must then list scenarios and run one scenario by name. The
Cucumber output and the skipped-step check already report each scenario.

### Install nextest with `cargo install`

Rejected. A source build adds compile time to each CI job without a tool cache.
The aqua backend installs the released binary.

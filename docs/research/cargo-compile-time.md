---
id: cargo-compile-time
kind: doc
title: Cargo compile-time investigation
---

# Cargo compile-time investigation

Research date: 2026-08-26.

## Conclusion

Do not change the shared Cargo profiles now. First measure one cold build and
one warm rebuild. The most useful local experiment is less debug information
for the `dev` and `test` profiles. Keep the current hosted CI cache and job
limits. They already match the current CI decision.

The release profile is not a compile-time target. It uses full LTO and one
code-generation unit for small release files. Cargo states that LTO adds link
time, and rustc states that one code-generation unit can compile more slowly.
This is an accepted size trade-off in
[[0015-size-optimized-release-profile|ADR-0015]].

## Current state

`Cargo.toml` has no `dev` or `test` profile settings. Cargo therefore uses
incremental compilation, 256 code-generation units, and full debug information
for normal local builds. Tests inherit the `dev` profile. Incremental data is
stored under `target/` and only applies to workspace members and path
dependencies. [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental)
[define these defaults](https://doc.rust-lang.org/cargo/reference/profiles.html#dev).

Hosted CI sets `CARGO_INCREMENTAL=0`, limits the core Cargo chain to two jobs,
uses `sccache`, and caches only downloaded Cargo inputs. This implements
[[0108-bounded-hosted-rust-compilation|ADR-0108]]. Do not enable incremental
compilation in those jobs. The ADR records that it does not combine well with
the CI compiler-result cache.

The workspace already keeps the Docker performance test package outside the
normal workspace. This avoids its dependency graph in normal workspace builds,
as ADR-0108 requires.

## Measurement plan

Use the same Rust toolchain, target directory, and command for each comparison.
Record an empty-target result and a result after one small edit in `src/`.

```sh
cargo build --timings
cargo test --workspace --timings
```

Cargo writes an HTML timing report with per-compilation duration and concurrency
data to `target/cargo-timings`. The report is for human use, not for machine
input. [Cargo build timing reports](https://doc.rust-lang.org/cargo/commands/cargo-build.html#compilation-options)

Run this baseline before each experiment:

```sh
cargo clean
cargo build --timings
# Change one small Rust implementation file.
cargo build --timings
```

Do not use `cargo clean` in the normal edit loop. It removes the incremental
data that Cargo reuses for warm rebuilds.

## Ranked experiments

### 1. Use line tables for local debug builds

Test this root manifest setting in a separate branch:

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.debugging]
inherits = "dev"
debug = true
```

`line-tables-only` keeps file names and line numbers for workspace backtraces
but omits variable and parameter information. The dependency override omits
their debug information. Full debug information is the current default for
`dev`, so this can reduce compiler and linker work. The `debugging` profile is
an opt-in full-debug path. Cargo documents this exact pattern and its faster
code generation, faster linking, and smaller `target` directory trade-offs.
[Cargo build performance guide](https://doc.rust-lang.org/cargo/guide/build-performance.html#reduce-amount-of-generated-debug-information)

Accept this only if the measured cold and warm times improve and the reduced
debugging detail is acceptable. This project uses Rust 1.97.1, so the setting
is available.

### 2. Use the local `sccache` cache for cold rebuilds

CI already uses `sccache`. A developer can also set `RUSTC_WRAPPER=sccache` in
a personal Cargo configuration. Cargo supports a `rustc-wrapper` command and
the `RUSTC_WRAPPER` environment variable. [Cargo build wrapper
configuration](https://doc.rust-lang.org/cargo/reference/config.html#buildrustc-wrapper)

Keep this as local setup, not a repository requirement. The sccache local cache
is 10 GB by default and only supports one server at a time. Concurrent servers
can cause build failures. [sccache local cache
rules](https://github.com/mozilla/sccache/blob/main/docs/Local.md)

Measure a clean rebuild after a branch switch. Do not expect a large gain for
one-file warm rebuilds. Cargo incremental compilation already targets that case.

### 3. Build the smallest relevant package set

The workspace includes the CLI, the Wasm helper, and the performance harness.
Use the CLI package for an ordinary CLI edit:

```sh
cargo test --package criv
```

Use `cargo test --workspace` before a shared-crate, manifest, or release change.
Cargo supports explicit package selection, so the smaller command does not
build unrelated workspace members. [Cargo package
selection](https://doc.rust-lang.org/cargo/commands/cargo-build.html#package-selection)

### 4. Do not reduce code-generation units before measurement

The default incremental value is already 256. More units allow more parallel
code generation and can reduce compile time, but can make generated code
slower. One unit can improve generated code but can compile more slowly.
[Cargo code-generation units](https://doc.rust-lang.org/cargo/reference/profiles.html#codegen-units)
[rustc code-generation units](https://doc.rust-lang.org/rustc/codegen-options/index.html#codegen-units)

Do not change this value for `dev` or `test` unless the timing report shows a
clear CPU-parallelism limit. Do not change the release value. ADR-0015 owns its
size result.

## Rejected changes

- Do not add LTO to `dev` or `test`. Cargo states that LTO increases link time.
- Do not increase `opt-level` to make compilation faster. Higher optimization
  levels can increase compiler time. [Cargo optimization
  levels](https://doc.rust-lang.org/cargo/reference/profiles.html#opt-level)
- Do not enable Cargo incremental compilation in hosted CI. ADR-0108 owns the
  selected `sccache` strategy.
- Do not cache the full `target/` directory in CI. ADR-0108 rejects this due to
  cache size and transfer time.

## Decision trigger

Create an ADR before changing `Cargo.toml`, `hk.pkl`, or the hosted workflow.
ADR-0015 governs the release profile. ADR-0108 governs CI compilation and
caching. A local-only developer guide can change without a new ADR if it does
not change those shared settings.

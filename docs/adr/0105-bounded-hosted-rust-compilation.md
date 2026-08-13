---
id: ADR-0105
kind: decision
title: Bounded hosted Rust compilation
status: accepted
date: 2026-08-13
supersedes:
  - ADR-0061
governs:
  - .github/workflows/ci.yml
  - .github/workflows/performance-notes.yml
  - .gitignore
  - Cargo.toml
  - hk.pkl
  - mise.toml
  - scripts/performance/container-test/Cargo.lock
  - scripts/performance/container-test/Cargo.toml
  - scripts/performance/container-test/tests/performance_container.rs
---

# Bounded Hosted Rust Compilation

## Context

[[0061-hook-owned-local-validation-and-direct-ci-profile|ADR-0061]] kept one
complete `hk check --all` core job and a separate `criv check` annotation run.
The complete profile starts its read-only steps in parallel. This behavior
caused several independent Cargo processes to use one hosted runner and one
build directory at the same time. CI logs showed package-cache and build-
directory lock waits while tests, Clippy, documentation checks, enforcement,
and Hawk competed for CPU.

The Cargo caches also included complete `target` directories. The active Linux
and Windows archives grew to several gigabytes. One Windows run spent more time
saving its archive than it spent building and testing. The repository's Cargo
and npm caches together also approached GitHub's default cache storage limit.

The direct annotation run repeated the same vault validation that the complete
hk profile performs. An empty-cache run spent more than two minutes in this
serial step before the profile started.

[[0069-repeatable-two-tier-performance-evidence|ADR-0069]] keeps the Docker-
dependent performance test explicit and ignored. Its Testcontainers dependency
still belonged to the root package, so ordinary workspace tests and Clippy had
to compile that unused dependency tree.

## Decision

Keep the local validation boundary from ADR-0061. Pre-commit and pre-push hooks
remain the normal local gates. Hosted core validation still invokes the complete
`hk check --all` profile directly, and `hk.pkl` remains the owner of every check
command.

Order the normal Cargo work in the hosted check profile. Workspace tests run
first, Clippy runs after the tests, the full criv vault check runs after Clippy,
and CI enforcement runs after the vault check. Hawk keeps its separate target
directory and can run beside that chain. Limit each of the two compiler chains
to two Cargo build jobs. Keep light format, workflow, and audit checks parallel.

The full criv check emits GitHub annotations when GitHub Actions runs it. Remove
the separate annotation build. Upload hk's machine-readable timing report on
success or failure so later CI changes have step-level evidence.

Do not cache complete Cargo target directories. Cache only downloaded Cargo Git
and registry inputs. Use pinned `sccache` compiler-result caching in every
required correctness lane that compiles Rust and in the asynchronous
performance-evidence workflow. Disable Cargo incremental compilation in those
workflows because an incremental Rust compilation is not reusable by `sccache`.
Do not put Hawk behind `sccache` until Hawk explicitly supports that compiler
wrapper.

Use one operating-system and lock-file key for each shared Cargo input cache.
Use one operating-system and lock-file key for the shared npm download cache.
The caches contain downloaded inputs, not job-owned build outputs, so companion
jobs do not need separate copies. The core job owns Linux cache publication.
Other Linux correctness lanes and Performance notes restore those inputs without
starting competing cache uploads. The Windows and macOS lanes own their separate
operating-system cache entries.

Cancel an older CI workflow when a newer commit updates the same pull request.
Do not cancel main-branch CI runs. Do not add this cancellation to Performance
notes because [[0070-publish-push-performance-evidence-as-git-notes|ADR-0070]]
requires evidence for each pushed commit.

Move the Docker performance integration test into an independent, locked
performance-container test package. Keep this package outside the root Cargo
workspace. The explicit `mise run perf-container` task selects its manifest.
Normal workspace tests, Clippy, and Hawk do not compile the Docker dependency
tree.

Keep every required lane and the stable `Repository checks` aggregate from
[[0103-required-repository-self-governance|ADR-0103]]. Keep Windows required as
decided by [[0084-require-windows-hosted-validation|ADR-0084]]. This decision
changes internal scheduling and cache ownership, not the validation surface.

## Consequences

A source-only CI run transfers compiler results instead of a multi-gigabyte
build directory. Cargo processes that share the normal target directory no
longer block one another. Hawk can still use a second compiler chain without
contending for all runner CPUs.

The first run with a new compiler or dependency set can remain slower while the
compiler-result cache fills. The hk timing artifact and `sccache` statistics
make this cost visible. A later split into separate hosted Rust jobs requires a
new decision because the complete profile remains one core job here.

The explicit performance-container task has the same behavior and pinned image,
but its dependency cost is absent from normal validation.

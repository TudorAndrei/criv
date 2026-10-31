---
id: ADR-0154
kind: decision
title: Cache Hosted Toolchains With Mise and Compiler Results With mbx
status: accepted
date: 2026-10-31
supersedes:
  - ADR-0108
  - ADR-0152
governs:
  - .github/actions/setup-build-tools/**
  - .github/workflows/ci.yml
  - .github/workflows/performance-notes.yml
  - .github/workflows/release.yml
  - .github/workflows/pages.yml
  - scripts/lint-workflows.sh
  - tests/ci-cache.sh
  - tests/hosted_release_scripts.sh
  - mise.toml
  - mise.lock
  - hk.pkl
  - .config/wt.toml
  - .worktreeinclude
  - .gitignore
  - Cargo.toml
  - scripts/performance/container-test/Cargo.lock
  - scripts/performance/container-test/Cargo.toml
  - scripts/performance/container-test/tests/performance_container.rs
---

# Cache Hosted Toolchains With Mise and Compiler Results With mbx

## Context

[[0108-bounded-hosted-rust-compilation|ADR-0108]] introduced bounded compiler
scheduling and sccache. [[0152-retain-local-cargo-cache-policy-in-standard-form|ADR-0152]]
kept mbx local and disabled it in CI. Some hosted lanes installed Rust directly,
and the mise action cached its own installation records without the toolchains
stored in the default `rustup` home. A restored record therefore did not avoid a
fresh Rust download.

The project now uses mbx for local compiler-result reuse. Hosted builds should
use the same cache owner rather than stack compiler wrappers. Tool installation
caching and compilation caching remain separate concerns.

## Decision

Use one shared setup action for hosted tool installation. Put the mise data
and `rustup` homes beneath the same runner-temporary directory, so mise-action
archives actual Rust toolchains, targets, and components. Keep Cargo's home
outside that directory, with only registry and Git downloads cached separately.
Preserve the caller's Cargo home and normalize native Windows paths.

Install configured tools through pinned mise-action. Remove separate workflow
`rustup` installation commands. The repository's mise configuration remains
the tool-version authority. Use explicit tool subsets for focused Rust lanes.
Keep both the primary compiler and Hawk's exact compiler where the full tool
set is needed. Hawk selects its compiler through `mise exec`, which selects
both the executable search path and the toolchain environment.

Replace sccache with pinned `jdx/mr-boxington-action`. Normal correctness and
performance-evidence builds use its GitHub objects cache mode, not full target
archives. Use an isolated hosted object store and per-job key suffixes; the
upstream generated keys and restore prefixes also distinguish OS, architecture,
and compiler identity. Export only the job's used or produced object closure.
Continue caching Cargo downloads because object bundles do not contain them.

Tool caches publish after successful installation only in default-branch push
runs. Compiler results publish after successful jobs only in those push runs.
Pull requests, manual runs, and release events restore without publishing.
Fork pull requests never publish. Core owns Linux Cargo and npm
download publication; the Windows and macOS watch jobs own their OS-specific
Cargo entries. Other lanes restore downloads without competing uploads.

Published release validation and native builds use the mbx local backend. They
may reuse tool installations and verified source downloads, but never import
or export shared compiler outputs. Keep the exact-commit release, auditable
binary, artifact, and verification boundaries unchanged.

Preserve the existing validation surface and scheduling: tests precede Clippy,
then vault validation and CI enforcement; Hawk uses a separate target directory.
The two compiler chains retain the core job's two-build-job limit. Keep light
checks parallel and retain the timing report. Keep incremental compilation
disabled in hosted workflows. Do not remove any required correctness lane or
change the stable Repository checks aggregate. Retain PR-only cancellation and
the separate performance-evidence workflow's publication behavior.

Locally, keep the pinned mbx Cargo wrapper, Worktrunk target copies, and
`MBX_TARGET_VIEWS=0`. Respect an explicit `MBX_DISABLE` in every environment,
rather than automatically disabling mbx in CI. Hawk sets that bypass and clears
`RUSTC_WRAPPER`. Do not adopt managed targets, a remote cache server, or an
external bucket. Retain the documented local rollback and measurement record.
The Docker performance test remains an independent, explicit test package.

Reference the shared action with GitHub's `$/` self-repository syntax, so it
comes from the workflow revision rather than mutable checkout contents. Until
pinned actionlint supports that syntax, translate only those uses lines in its
input stream to the local-path spelling. This keeps its action/input checks;
zizmor audits the actual original workflows and composite action.

## Consequences

A tool-cache hit can restore Rust itself, not just its installation record. Ordinary Cargo
commands use one compiler cache owner on developers' machines and in CI.
A compiler-cache miss still performs a normal compilation. First runs after
this policy change need to fill new cache keys from a trusted main-branch push.

Toolchain archives and object bundles can still be large. Avoiding whole target
archives is not a claim of faster builds or lower storage use. Inspect hosted
cache-hit logs, payload sizes, timing reports, and compiler summaries before
claiming a performance improvement.

Release compiler outputs remain isolated from shared caches. Existing accepted
ADRs remain unchanged; this successor preserves their scheduling, validation,
local worktree, and release boundaries while replacing their cache policy.

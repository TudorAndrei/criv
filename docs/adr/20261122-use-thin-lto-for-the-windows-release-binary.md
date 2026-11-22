---
id: ADR-20261122-use-thin-lto-for-the-windows-release-binary
kind: decision
title: Use thin LTO for the Windows release binary
status: accepted
date: 2026-11-22
governs:
  - .github/workflows/release.yml
---

# Use thin LTO for the Windows release binary

## Context

[[0015-size-optimized-release-profile|ADR-0015]] sets fat LTO, one codegen
unit, and `opt-level = "z"` for release builds. In the release run for commit
`a8dedf4`, the Windows x86_64 build took 792 seconds. The Linux x86_64 build
took 87 seconds. The Windows build was the longest job in the release.

## Decision

The release workflow builds the Windows x86_64 binary with
`CARGO_PROFILE_RELEASE_LTO=thin`. The Linux and macOS builds keep fat LTO.
The other ADR-0015 settings stay the same for all targets.

## Consequences

The Windows release build is faster. The Windows binary can be a little larger
than with fat LTO. A local `cargo build --release` on any platform still uses
fat LTO from `Cargo.toml`.

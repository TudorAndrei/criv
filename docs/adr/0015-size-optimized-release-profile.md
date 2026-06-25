---
id: ADR-0015
kind: decision
title: Size Optimized Release Profile
status: accepted
date: 2026-05-15
governs:
  - Cargo.toml
---

# Size Optimized Release Profile

## Context

criv release binaries are intended for direct download and package-manager
installation, as described by [[0014-tag-triggered-release-binary-workflow|ADR-0014]] and [[releasing]]. A default Rust
release build on macOS produced an 18,754,544 byte `criv` binary.

The upstream `johnthagen/min-sized-rust` guidance recommends first applying
stable Cargo release-profile settings before considering nightly-only,
platform-specific, or behavior-heavy techniques. For criv, the relevant stable
knobs are symbol stripping, size-oriented optimization, link-time optimization,
a single codegen unit, and aborting rather than unwinding on panic.

## Decision

Use a workspace release profile in `Cargo.toml` that strips symbols, sets
`opt-level = "z"`, enables LTO, uses one codegen unit, and sets
`panic = "abort"`.

`opt-level = "z"` is chosen from measurement rather than assumption. With the
same stable size profile, `opt-level = "z"` produced a 9,815,840 byte binary on
macOS, while `opt-level = "s"` produced an 11,449,312 byte binary.

Do not use nightly `build-std`, `panic=immediate-abort`, `#![no_main]`,
`#![no_std]`, or UPX for the normal release path. Those techniques may reduce
size further, but they add toolchain instability, portability risk, operational
surprise, or a substantially different programming model.

## Consequences

Release artifacts are substantially smaller without changing the published
archive names or the tag-triggered release workflow from [[0014-tag-triggered-release-binary-workflow|ADR-0014]].

Release builds become slower because LTO and a single codegen unit reduce
parallel compilation. Panics in release builds abort instead of unwinding, so
cleanup that relies on unwinding will not run after a panic. This is acceptable
for the CLI release path because panics represent defects, not normal control
flow.

Future binary-size work should start with dependency and feature analysis before
moving to nightly or `no_std` approaches.

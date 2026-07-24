---
id: ADR-0043
kind: decision
title: Hawk visibility analysis
status: accepted
date: 2026-07-23
governs:
  - Cargo.toml
  - criv.toml
  - hawk.toml
  - mise.toml
  - hk.pkl
  - .github/workflows/ci.yml
  - extensions/vscode-criv/package.json
  - src/**
  - crates/**
---

# Hawk Visibility Analysis

## Context

Criv ships a Rust CLI binary, while its workspace is primarily library-style
source. Rust's built-in lints do not determine which `pub` declarations are
needed across the complete workspace product surface. That leaves unnecessary
public APIs and dead public declarations outside the existing formatting,
Clippy, and test checks.

[[0013-mise-managed-hk-hook-toolchain|ADR-0013]] established mise and hk as the
repository tooling front door. The `criv-wasm` workspace crate exports
functions that `wasm-pack` builds for the Obsidian and VS Code companions, so
those exports are outside the CLI binary's Cargo reachability graph.

## Decision

Pin Rust 1.97.1 and `cargo-hawk` 0.1.9 through `mise.toml`. Hawk requires that
exact Rust toolchain because it uses compiler internals.

Declare `criv` as Hawk's sole production binary in `hawk.toml`. Run
`mise run hawk` as part of the full `check` hook for Rust, Cargo, and Hawk
configuration changes, and deny all enabled Hawk warnings. Keep it out of the
pre-commit hook because it performs whole-workspace compiler analysis.

Invoke Hawk with `--target-dir target/hawk` and `--exclude-crate criv_wasm`.
The dedicated target directory keeps its instrumented compiler artifacts apart
from the other parallel checks. Its public WASM exports are consumed by
generated artifacts instead of the CLI binary, so treating that crate as an
internal CLI library would produce false positives.

## Consequences

The full repository check now rejects unnecessary or dead public APIs in the
CLI product surface. Visibility changes must preserve all CLI, test, example,
and doctest consumers modeled by Hawk.

Updating Hawk requires updating its matching Rust toolchain at the same time.
The project Rust pin also governs CI and the VS Code WASM build command, keeping
all workspace Rust compilation on the same version.

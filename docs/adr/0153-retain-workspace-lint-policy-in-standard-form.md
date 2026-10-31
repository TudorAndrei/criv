---
id: ADR-0153
kind: decision
title: Retain Workspace Lint Policy in Standard Form
status: accepted
date: 2026-10-30
supersedes:
  - ADR-0144
governs:
  - Cargo.toml
  - crates/criv-state-wire/Cargo.toml
  - crates/criv-wasm/Cargo.toml
  - scripts/performance/Cargo.toml
  - hk.pkl
  - mise.toml
---

# Retain Workspace Lint Policy in Standard Form

## Context

[[0144-apply-strict-lints-across-the-workspace|ADR-0144]] has no Context
heading. Preserve its accepted bytes while carrying its complete policy into
the format required by
[[0148-separate-markdown-formatting-from-vault-validation|ADR-0148]].

## Decision

Keep strict Clippy and Rust lint configuration in workspace.lints. The CLI,
State wire crate, and Wasm crate inherit it. Keep `unsafe_code` forbidden for
these crates, with no local allowance able to override it.

Test builds may relax panic and pedantic lints through crate-level test
configuration and clippy.toml. Production exceptions use expect attributes
with a reason, so a stale exception is visible.

The performance harness keeps its separate lint configuration because process
measurement needs OS APIs and numeric conversions. Deny unsafe code except at
explicit reasoned crate-level exceptions; require documented unsafe blocks
and reject untracked allow attributes.

The hk Clippy step checks the workspace on the host and the Wasm library on
wasm32-unknown-unknown. Mise installs the target and the Clippy and `rustfmt`
components for the pinned toolchain. Updating that development toolchain does
not automatically raise the Cargo manifests' minimum supported Rust version.

## Consequences

New workspace crates inherit the strict set or need their own decision for an
exception. Wasm is checked in both host and target builds. The latest compiler
can expose new lint findings, which must be repaired without weakening the
policy. No accepted historical record is rewritten.

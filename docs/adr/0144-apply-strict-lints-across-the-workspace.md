---
id: ADR-0144
kind: decision
title: Apply strict lints across the workspace
status: accepted
date: 2026-10-07
governs:
  - Cargo.toml
  - crates/criv-state-wire/Cargo.toml
  - crates/criv-wasm/Cargo.toml
  - scripts/performance/Cargo.toml
  - hk.pkl
  - mise.toml
---

# Apply strict lints across the workspace

Before this decision, the strict Clippy set was a `[lints.clippy]` table in the
root `criv` package. Cargo applies a package lint table only to that package.
`criv-state-wire` and `criv-wasm` ship code to the CLI and to the editors, but
they used the default Clippy set. They had unchecked `expect`, slice indexing,
`as` casts, and unchecked arithmetic. Clippy did not check the `wasm32` build of
`criv-wasm` at all, so code behind `cfg(target_arch = "wasm32")` was not linted.

## Decision

Keep the lint set in `[workspace.lints]` in the root `Cargo.toml`. The root
package, `criv-state-wire`, and `criv-wasm` inherit it with `[lints] workspace =
true`. The set has the earlier strict Clippy lints and
`unsafe_code = "forbid"`. None of these crates contains `unsafe` code, and
`forbid` stops a local `allow` from adding it.

A test build may relax the panic and pedantic lints with one crate-level
`cfg_attr(test, allow(...))`. `clippy.toml` keeps the `allow-*-in-tests`
settings. Production code uses `#[expect(lint, reason = "...")]` for each
exception. A plain `#[allow]` does not tell when it becomes stale.

The performance harness in `scripts/performance` does not inherit the workspace
set. It measures processes and must call OS APIs such as `wait4`, `statvfs`,
and `GetProcessMemoryInfo`, and its statistics need float casts. It keeps the
default Clippy set and adds:

- `unsafe_code = "deny"`, with one crate-level `#[expect(unsafe_code, reason)]`
  in each binary that calls an OS API;
- `clippy::undocumented_unsafe_blocks = "deny"`, so every block keeps its
  `SAFETY:` comment;
- `clippy::allow_attributes = "deny"`, so every exception is an `expect` with a
  reason.

The `cargo-clippy` step in `hk.pkl` also runs Clippy on `criv-wasm` for
`wasm32-unknown-unknown`. `mise.toml` installs that target and the `clippy` and
`rustfmt` components with the pinned toolchain.

## Consequences

A new workspace crate gets the strict set when it adds `[lints] workspace =
true`. A crate that cannot accept it needs its own decision, as the harness has
here.

The pre-push hook and CI now build `criv-wasm` twice for Clippy, once for the
host and once for `wasm32`. The extra build is small next to the root crate.

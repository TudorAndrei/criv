---
id: ADR-0173
kind: decision
title: Observe Discovery Through a Feature-Gated Selection API
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0072
governs:
  - Cargo.toml
  - src/**
  - scripts/performance/**
  - scripts/measure-performance.sh
  - .github/workflows/performance-notes.yml
---

# Observe Discovery Through a Feature-Gated Selection API

## Context

[[0072-keep-performance-observation-outside-core|ADR-0072]] keeps performance
observation out of `src/` and rejects a Cargo feature for instrumentation.

The file-discovery probe did not follow that boundary. `src/lib.rs` included
`scripts/performance/discovery_probe.rs` through a `#[path]` module and added
three test-only functions for it. The probe had three `#[ignore]` tests that
`criv-discovery-baseline` started by test name. `criv-discovery-adapter` copied
the probe into an exported v0.9.0 tree, applied
`scripts/performance/adapters/v0.9.0-discovery-probe.patch`, and searched the
text of `src/lib.rs` to confirm the hooks. So the core crate compiled
measurement code, and a harness change required a change to `src/lib.rs`.

The adapter could not build after the LikeC4 removal. The shared probe expects
one Markdown list from the Vault hook, but the v0.9.0 patch returns Markdown
and C4 lists. The release gate that used the v0.9.0 rows was retired with
[[0117-hosted-automatic-release-acceptance|ADR-0117]] and its successors.

## Decision

Retain the complete decision of ADR-0072, with one change. ADR-0072 rejects
every Cargo feature in the core crate. This decision allows one feature that
exposes selection results and contains no measurement code.

The core crate has a `selection` Cargo feature. It is off by default. With the
feature, `criv::selection` exposes three functions that return the files criv
selects for a repository root: `source_files` runs the complete Source build,
`source_candidates` runs only the Source candidate walk, and `vault_markdown`
lists the Vault Markdown notes. They call the same code as the CLI. They do
not measure, count, time, or print.

All observation stays in `criv-perf-harness`. Its `criv-discovery-probe`
executable calls one selection function, reads the selected file sizes,
computes the path digests, and prints the `criv.discovery-probe.v1` line.
`criv-discovery-baseline` builds that executable in the release profile and
runs it once for each sample. The probe output schema is unchanged.

Build the measured `criv` binary in a Cargo invocation that does not also build
`criv-perf-harness`. Cargo unifies features across packages in one invocation,
so a combined build would enable `selection` in the measured binary. Release
builds name only the `criv` package.

Remove `criv-discovery-adapter`, the v0.9.0 adapter patch, the Markdown probe
profile, and the adapter receipt. The v0.9.0 rows that were recorded stay
valid as recorded evidence. To reproduce them, check out a revision before
this decision, where the adapter and its probe still match.

## Consequences

`src/` contains no probe, no `#[path]` module into `scripts/`, and no test
entry point for the harness. The selection functions are a small library
interface. A refactor that keeps their signatures does not change the harness.

The default and release `criv` artifacts do not contain the selection module.
A workspace build or test enables it, because the harness is a workspace
member.

A comparison with an old criv revision needs a probe built for that revision.
This decision does not keep a tool for that.

---
id: ADR-0065
kind: decision
title: Shared Compiled Policy Scan Plans
status: accepted
date: 2026-08-03
governs:
  - src/*.rs
  - scripts/measure-performance.sh
---

# Shared Compiled Policy Scan Plans

## Context

[[0040-inline-only-adr-policy-rules|ADR-0040]] makes each ADR policy entry an
inline ast-grep definition, and
[[0056-adr-policy-patterns-are-the-only-registered-patterns|ADR-0056]] gives
those entries stable ADR-owned IDs. `criv check` and `criv enforce` implement
that policy lifecycle with separate planning loops.

Both loops currently visit every accepted note, resolve its effective
`governs:` scope, clone the resolved file set for every policy entry, compile
each candidate to decide whether it is executable, and then ask the structural
batch scanner to compile it again. Check validation compiles the same
definition once more to report malformed policy diagnostics.

The repository currently has 64 accepted ADRs, but only three own executable
policies and those ADRs contain four entries in total. The common accepted ADR
therefore has no policy work to contribute. Resolving scopes for all 64 ADRs is
not an inherent cost of enforcing four policies; it follows from the order of
the current transforms and from duplicating that order in two commands.

## Decision

Use one typed policy-scan planner for check and enforcement. It consumes the
vault's ordered notes and source catalog as a batch and produces typed policy
definition diagnostics plus typed policy violations for the calling command to
render.

The planner validates and compiles policy entries before resolving owner
scopes. A missing or empty ID, incomplete or ambiguous inline definition,
unsupported language, or ast-grep compile failure produces a typed diagnostic
and no scan request. Duplicate IDs retain the existing duplicate diagnostic and
otherwise-executable scan behavior. Only an exact `accepted` owner with at
least one executable entry advances to scope planning.

Resolve each advancing ADR's effective `governs:` scope exactly once per
command. Deduplicate that result into one owner path set and share it across all
compiled policies owned by the ADR. Commit and push enforcement intersect the
owner set with their changed-file set once per owner; check and CI enforcement
use the complete owner set.

Represent successful structural compilation explicitly. A compiled policy
holds its parsed language and ast-grep matcher for the lifetime of the command,
and structural batch requests borrow that value instead of accepting a raw
policy definition. Each definition is compiled once and each affected source
file remains parsed once per batch. State generation and explicit policy search
use the same compiled structural interface rather than preserving a parallel
raw request path.

`src/policy_scan.rs` owns selection, definition outcomes, per-owner grouping,
scope resolution, changed-file filtering, and deterministic violation records.
`src/check.rs` adapts its typed diagnostics and violations into the existing
check codes, messages, severities, paths, lines, ordering, and output formats.
`src/enforce.rs` supplies the optional changed-file set and adapts violations
into the existing enforcement lines. Neither caller owns policy planning.

This decision does not change policy syntax, exact accepted-status activation,
search lookup or scope behavior, generated-state registration, incremental
state reuse, `criv.state.v0`, or companion behavior. Invalid source reads and
structural scans remain command errors. The planner is command-local; it adds no
persistent cache, configuration, parallel execution path, or dependency.

Deterministic work counts test compilation and owner-scope resolution. The
existing local performance harness records repeated whole-command samples and
reports their distribution. Exact counters prove the removed work; wall-clock
samples remain supporting evidence because they also include markdown, C4,
filesystem, and process-start costs.

## Consequences

An accepted ADR without an executable policy performs no policy-scope
resolution. Each relevant ADR contributes one path set regardless of its number
of policies, and each valid definition contributes one compiled matcher for the
command. On the current repository that means three owner scope resolutions and
four compiled matchers rather than scope work for all 64 accepted ADRs and
repeated compilation across validation and scanning.

Check and enforcement gain locality at one policy-scan seam while retaining
their distinct adapters. `src/check.rs` becomes smaller because policy
definition validation and scanning leave the general validation module; its
unrelated markdown, C4, and output-rendering responsibilities are not split by
this decision.

The command retains compiled matcher and owner path memory until the scan
finishes. That memory is bounded by executable policy owners and replaces the
previous per-policy path-set clones. Migrating state generation to the compiled
structural interface increases the refactor surface, but keeps one compilation
contract and avoids two scan representations that could drift.

---
id: performance
kind: doc
title: Repeatable Performance Evidence
targets:
  symbols:
    - scripts/measure-performance.sh
    - src/measurement.rs
---

# Repeatable Performance Evidence

Performance claims in criv use deterministic work evidence plus repeated timing
samples from an explicit release binary. One timing run on the development
checkout is useful for exploration but is not project evidence.

## Canonical workloads

The canonical workload set has two maintainer-approved shapes derived from
observed criv vaults:

| Manifest | Tier | Notes | Source files | Source bytes | Links and references | Policies | C4 artifacts | Changed sources |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `barrs-small.toml` | small | 23 | 12 | 273,948 | 3 | 0 | 0 | 1 of 12 |
| `criv-medium.toml` | medium | 77 | 119 | 1,291,218 | 212 | 4 | 4 | 1 of 119 |

The checked-in manifests retain repository revision, note split, language/file
extension distribution, symbols, link categories, and exact changed fraction.
Generated workload content is sanitized and deterministic; it reproduces the
shape, not proprietary text or source.

A large tier is deliberately absent. No observed large criv vault was available
for approval on 2026-08-03, and extrapolating either current shape would create
the invented average prohibited by the evidence policy. Adding a large tier
requires a separately reviewed observed manifest.

Docker is an optional execution environment, not a workload. An explicitly
invoked [Testcontainers for Rust](https://rust.testcontainers.org/) lane builds
and runs the harness inside a digest-pinned image, then supplies these same
manifests and generated vaults. A future observed large manifest can use that
lane without making the container image, an arbitrary checked-out codebase, or
container storage into the vault definition.

## Required run identity

Harness runs require an explicit executable path and profile identity. Evidence
records the canonical binary path and BLAKE3 digest, repository revision and
dirty status, Cargo profile, `rustc --version --verbose`, operating system,
release, architecture, processor model when available, UTC start time, harness
schema, workload manifest bytes and digest, command case, declared cache state,
and sample count.

Results from different binaries, workload manifests, profiles, or cache-state
contracts are not combined. Machine differences remain visible metadata rather
than being normalized into a claim of universal performance.

Containerized evidence additionally records the immutable image digest, Docker
server identity, and container architecture. The release binary is built
inside that pinned environment; a macOS or Windows host executable is never
copied into a Linux container and mislabeled as the same artifact. Host and
container rows are distinct machine identities and are not merged.

## Sampling and isolation

Every command case declares `cold` or `warm` initial local state. A cold sample
starts from a newly generated vault with no `.criv/`. A warm sample starts from
another newly generated vault, performs its declared untimed seed command, and
then measures the requested command. Each timed sample gets a separate vault;
no sample inherits `.criv` files or generated outputs from another sample.

An untimed disposable warm-up precedes the recorded samples for each workload
and case. The default sample count is five and the minimum evidence count is
three. Failed commands remain raw sample rows with exit status and captured
output references, make the harness fail, and are excluded from successful
timing summaries rather than disappearing.

## Raw and summarized evidence

Each run writes a new result directory. `samples.jsonl` contains one row per
sample, preserving identity, cache state, exit status, elapsed/user/system
seconds, output digests, generated-state and snapshot hashes when present, and
the opt-in structured work record. `summary.json` groups compatible successful
rows and reports sample count, minimum, median, maximum, and median absolute
deviation without discarding raw values.

Cross-commit comparisons use separate result directories and freshly generated
vaults. They compare rows only when workload digest, command case, cache state,
sample count, profile, and machine identity match. Results are local artifacts
and stay outside source control.

## Structured work and semantic parity

The harness may opt into command-local coarse spans and deterministic work
counters through a harness-provided output path. Required counters cover note
and source bytes read, files parsed and reused, source resolutions, policies
compiled, AST parses, State partitions and serializations, cache bytes, and
published output bytes. Per-element timing is excluded because observer cost
would dominate the small operations it attempts to explain.

Canonical harness runs collect the structured record by default. Use
`--without-measurement` only to produce the uninstrumented half of an explicit
semantic-parity comparison; the run identity records which mode was used.

Instrumentation is disabled by default. Enabling it must preserve exit status,
stdout, stderr, generated State bytes, snapshot hash, and graph-cache bytes for
successful and failing commands. Deterministic counters are primary evidence
for removed work; repeated wall-clock samples are supporting evidence.

## Validation boundary

Performance runs are deliberately absent from normal hk hooks and hosted CI,
as established by
[[0049-checks-defined-in-hk-not-mise|ADR-0049]]. Harness and generator smoke
tests validate their contracts, while contributors invoke measurements
explicitly through `mise run perf`.

The Docker/Testcontainers lane is ignored by default and has its own explicit
entry point because it requires a Docker-API-compatible runtime and may need to
acquire the pinned image. Testcontainers owns container startup and cleanup;
the harness still owns sample isolation inside the container.

The governing decision is
[[0069-repeatable-two-tier-performance-evidence|ADR-0069]].

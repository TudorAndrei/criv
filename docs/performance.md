---
id: performance
kind: doc
title: Repeatable Performance Evidence
targets:
  symbols:
    - scripts/measure-performance.sh
---

# Repeatable Performance Evidence

Performance claims in criv use correctness invariants plus repeated external
timing samples from an explicit release binary. One timing run on the
development checkout is useful for exploration but is not project evidence.

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
seconds, output digests, generated-state and snapshot hashes when present.
`summary.json` groups compatible successful rows and reports sample count,
minimum, median, maximum, and median absolute deviation without discarding raw
values.

Cross-commit comparisons use separate result directories and freshly generated
vaults. They compare rows only when workload digest, command case, cache state,
sample count, profile, and machine identity match. Results are local artifacts
and stay outside source control.

## Core boundary

The harness observes `criv` only as a subprocess. Core code contains no
performance environment protocol, spans, counters, or measurement artifact
writer, and the harness uses the same ordinary release binary distributed to
users. Process resource usage and output identities are the complete runtime
observation surface.

Correctness tests may count work behind `cfg(test)` to prove invariants such as
partition reuse or a single source enumeration. Those assertions are compiled
only for tests and are not a runtime measurement API. Repeated timing samples
support such claims but do not identify internal operations on their own.

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

## Push notes

Every successful repository push runs a separate, non-gating host measurement
workflow and publishes its compact JSON summary to
`refs/notes/criv-performance`. The complete result directory remains available
as a workflow artifact for 30 days. Fetch and inspect the durable note with:

```sh
git fetch origin refs/notes/criv-performance:refs/notes/criv-performance
git log --notes=criv-performance
git notes --ref=criv-performance show <commit>
```

The publisher uses the workflow's scoped token, never force-pushes the notes
ref, and retries after concurrent note updates. The job is not part of the CI
aggregate or local hooks and does not require Docker.

The governing decision is
[[0072-keep-performance-observation-outside-core|ADR-0072]].

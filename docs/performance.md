---
id: performance
kind: doc
title: Repeatable Performance Evidence
targets:
  symbols:
    - scripts/measure-performance.sh
    - scripts/performance/src/bin/criv-perf-report.rs
---

# Repeatable Performance Evidence

Performance claims in criv use correctness invariants plus repeated external
timing samples from an explicit release binary. One timing run on the
development checkout is useful for exploration but is not project evidence.

## Canonical workloads

The canonical workload set has two shapes from observed criv vaults and two
deterministic Elixir acceptance shapes:

| Manifest | Tier | Notes | Source files | Source bytes | Links and references | Policies | C4 artifacts | Changed sources |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `barrs-small.toml` | small | 23 | 12 | 273,948 | 3 | 0 | 0 | 1 of 12 |
| `criv-medium.toml` | medium | 77 | 119 | 1,291,218 | 212 | 4 | 4 | 1 of 119 |
| `elixir-mixed.toml` | medium | 12 | 24 | 524,288 | 32 | 1 | 1 | 1 of 24 |
| `elixir-parse-heavy.toml` | large | 4 | 128 | 4,194,304 | 8 | 0 | 0 | 1 of 128 |

The checked-in manifests retain repository revision, note split, language/file
extension distribution, symbols, link categories, and exact changed fraction.
Generated workload content is sanitized and deterministic; it reproduces the
shape, not proprietary text or source.

The mixed Elixir workload contains `.ex`, `.exs`, Rust, and TypeScript files.
The parse-heavy workload contains 96 `.ex` files and 32 `.exs` files with
8,192 callables. Every successful sample proves that each selected Elixir path
and byte entered the Elixir source graph. These workloads record cost. They do
not set an Elixir speed limit.

The large representative file-discovery workload is the observed
`flowcopilot/ouro` checkout. Evidence identifies its exact revision and full
worktree inventory, including ignored generated files. The local inventory is
content-addressed and is not committed because it contains repository paths.
A strict APFS copy-on-write snapshot isolates each command run on the
controlled macOS host. The runner resets tracked files, the Git index, its
known live-test files, and generated criv State before each sample. Synthetic
9,000, 90,000, and 225,000-file trees test scaling and edge cases only. They
are not representative user-project evidence.

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
sample. Each row keeps identity, cache state, exit status, elapsed, user and
system time, peak resident memory, selected source files and bytes, Elixir
parse coverage, output digests, generated-state hashes, and snapshot hashes
when present.
`summary.json` groups compatible successful rows and reports sample count,
minimum, median, maximum, and median absolute deviation without discarding raw
values. `report.html` is a self-contained derived view with shared-scale timing
ranges, exact-value tables, run identity, and workload provenance. It uses no
external assets or services, and JSON remains the canonical evidence.

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
`refs/notes/criv-performance`. The GitHub job summary shows the headline values
and links to a 30-day workflow artifact containing `report.html` alongside the
complete raw result directory. The note names the artifact and report path.
Fetch and inspect the durable note with:

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

## Release boundary

Performance observation does not approve or block an automatic release. The
Performance notes workflow records a compact host measurement after a push.
Explicit `mise run perf` runs can use the full synthetic workload set or Ouro.
The production CLI has no measurement API.

Automatic release runs deterministic quality and native archive checks. It
builds one binary for each supported target, records its SHA-256 digest in
`release-manifest.json`, verifies the archive on that target, and publishes the
same bytes. It does not generate large fixtures, build a comparison baseline,
or collect repeated samples. See
[[0121-separate-performance-evidence-from-automatic-release|ADR-0121]] and
[[releasing]] for the release sequence.

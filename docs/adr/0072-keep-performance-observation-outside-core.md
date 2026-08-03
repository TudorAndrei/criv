---
id: ADR-0072
kind: decision
title: Keep Performance Observation Outside Core
status: accepted
date: 2026-08-03
supersedes:
  - ADR-0069
  - ADR-0070
governs:
  - src/**
  - scripts/performance/**
  - scripts/measure-performance.sh
  - .github/workflows/performance-notes.yml
---

# Keep Performance Observation Outside Core

## Context

[[0069-repeatable-two-tier-performance-evidence|ADR-0069]] established isolated
small and medium workloads, repeated samples, optional Testcontainers execution,
and command-local spans and counters compiled into the core `criv` binary.
[[0070-publish-push-performance-evidence-as-git-notes|ADR-0070]] then made those
counters part of every successful push note.

The workload and sampling boundaries are useful, but performance measurement is
not a core `criv` responsibility. Disabled instrumentation still couples normal
commands to the harness protocol, adds code and no-op calls to release binaries,
and makes benchmark schema changes touch production algorithms. A feature flag
would remove that code from one artifact while retaining the same architectural
coupling in `src/`.

The maintainer requires a stronger boundary: production code must not know that
it is being benchmarked.

## Decision

Keep performance observation entirely outside `src/`. Remove the measurement
module, performance environment protocol, spans, counters, and measurement
publication from the core crate. Do not replace them with a Cargo feature or a
second instrumented core binary. Test-only counters that prove algorithmic
invariants may remain behind `cfg(test)`; they are correctness seams and are not
an exported runtime measurement protocol.

The `criv-perf-harness` remains a separate, non-publishable workspace package.
It launches an ordinary release `criv` subprocess and observes only process and
artifact boundaries: elapsed, user, and system time; exit status; captured
stdout and stderr digests; generated State and source-graph digests; and snapshot
identity. Each sample still receives a newly generated vault and declared cold
or warm setup. Run metadata still identifies the exact binary, repository,
machine, manifest, case, cache state, and sample count.

Retain the two canonical observed workload shapes from ADR-0069: `barrs-small`
and `criv-medium`. Continue to omit a synthetic large tier. Adding a large tier
requires a separately reviewed manifest derived from an observed vault. Keep
Testcontainers as an explicit Docker-compatible execution environment using the
same manifests; it is not a workload or vault source.

Retain five samples by default and a minimum of three for project evidence.
Preserve raw JSONL rows and median, minimum, maximum, and median absolute
deviation summaries. Failed commands remain raw evidence and fail the harness.
Comparisons remain valid only across matching workload, case, cache, profile,
binary, and machine identities.

Continue the non-gating push workflow and `refs/notes/criv-performance` ref from
ADR-0070. Publish note schema v2 with run identity and external timing summaries,
but no internal work counters or spans. Upload the complete raw result directory
for 30 days. The notes publisher retains scoped `contents: write` permission,
bounded non-fast-forward retries, and no force push.

Use correctness tests, including incremental partition-allocation and source
lifecycle assertions, to prove reduced internal work. External repeated timings
support those claims but do not pretend to identify internal operations.

## Consequences

Release binaries contain no performance-measurement implementation or dormant
harness protocol. Core algorithms can change without maintaining a benchmark
observer API, and the harness measures the same artifact users receive.

Git notes lose deterministic internal counters and therefore cannot by
themselves explain why a timing changed. Raw output identities, isolated samples,
and correctness tests keep performance claims auditable without assigning
benchmarking responsibility to the application.

The harness remains substantial tooling and the hosted push workflow still
consumes build time. That cost is isolated under `scripts/performance`, tests,
fixtures, and workflow configuration rather than the core crate.

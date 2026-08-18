---
id: ADR-0118
kind: decision
title: Bound hosted Source release scaling
status: accepted
date: 2026-08-18
supersedes:
  - ADR-0117
governs:
  - .github/workflows/release.yml
  - scripts/performance/assemble-hosted-release-gates.sh
  - scripts/performance/src/bin/criv-discovery-gate.rs
  - tests/hosted_release_scripts.sh
---

# Bound Hosted Source Release Scaling

## Context

[[0117-hosted-automatic-release-acceptance|ADR-0117]] requires a matched
250,000-entry Source comparison on hosted macOS. The v0.9.0 baseline has a
fixed 30-second `fff` indexing limit. It timed out in every 225,000-file Source
and Source-candidate sample in automatic release run `32068123021`. The
candidate did not run because the failed baseline stopped the workflow.

A matched performance ratio needs successful baseline and candidate samples.
The published v0.9.0 implementation cannot supply that baseline on the hosted
macOS runner. Changing its timeout in the test adapter would measure changed
baseline behavior.

## Decision

Keep every ADR-0117 release rule except the 250,000-entry Source comparison.
Run the matched 100,000-entry Source and Source-candidate workloads on Linux
x86_64, Linux ARM64, macOS ARM64, and Windows x86_64. Keep their selected-path
identity, five-sample stability, 50 percent time ratio, and 110 percent peak
memory gates.

Do not run the 250,000-entry Source workload as a release gate while v0.9.0 is
the baseline. Keep the 250,000-entry Vault and Markdown workloads on hosted
macOS. Keep the hosted live-watch, artifact, build, dependency, receipt, tag,
and publication rules from ADR-0117.

A future release can restore a larger matched Source gate when the accepted
baseline can complete it on the hosted runner.

## Consequences

The automatic release can finish without changing v0.9.0 behavior in a test
adapter. Large Source scaling still has cross-platform coverage at 90,000
selected files. Vault and Markdown keep 225,000-file coverage on hosted macOS.

## Alternatives Considered

### Increase the v0.9.0 adapter timeout

Rejected. The adapter would hide a real v0.9.0 failure and change the baseline
that the evidence claims to measure.

### Remove all 250,000-entry workloads

Rejected. The v0.9.0 Vault and Markdown profiles can complete those workloads,
so their matched large-project evidence stays useful.

### Keep retrying the unchanged workflow

Rejected. Two hosted runs reached the same fixed baseline timeout. A retry
cannot make a hard 30-second product limit reliable.

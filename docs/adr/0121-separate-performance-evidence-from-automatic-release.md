---
id: ADR-0121
kind: decision
title: Separate performance evidence from automatic release
status: accepted
date: 2026-08-20
supersedes:
  - ADR-0120
governs:
  - .github/workflows/ci.yml
  - .github/workflows/release.yml
  - .github/workflows/performance-notes.yml
  - scripts/package-release-assets.sh
  - tests/hosted_release_scripts.sh
---

# Separate Performance Evidence from Automatic Release

## Context

[[0120-reset-release-baseline-for-default-elixir-support|ADR-0120]] keeps large
matched workloads, repeated clean builds, and performance acceptance in every
automatic release. Automatic release run `32369457743` spent between 30 and 71
minutes in each native measurement lane. All four lanes passed. The aggregate
gate then rejected one result after 72 minutes. The run did not print the failed
check or upload its receipt.

This sequence tests performance too late and makes release publication depend
on noisy hosted-runner measurements. The separate Performance notes workflow
already records non-gating observations for each push. Normal CI and the exact
release quality job own correctness.

## Decision

Do not run performance workloads, baseline builds, repeated build samples, or
a performance gate in Automatic release. Do not publish a release-gate Git
note. Performance notes and explicit `mise run perf` measurements remain
available, but they do not approve or block a release.

Keep the automatic version selection and resumable prepared-release behavior.
Run the complete exact-commit quality job. Build the CLI once on each supported
native release host. Upload those exact binaries and package them without a
rebuild.

The package step creates `release-manifest.json` with the release commit,
version, target, archive, byte size, and SHA-256 digest for each binary. Native
jobs verify the archive checksums, binary digest, version, one-shot State
publication, and operation without a Git executable. Publish and attest only
after every native verification passes.

The current workflow code can package an older prepared release. The binary and
VS Code package still come from the exact prepared release commit. This keeps
old untagged release commits resumable without requiring their older release
orchestration scripts.

Keep Elixir completeness and source-discovery correctness in deterministic
workspace and CI tests. A performance result cannot permit criv to skip a
selected source file.

## Consequences

Automatic release no longer waits for synthetic 100,000-file and 250,000-file
workloads or six clean builds per host. Its long path is one native release
build plus the existing exact-commit quality and archive checks.

A hosted performance regression does not stop publication. Performance notes
remain visible for investigation, and a regression fix can still use the full
performance harness. Release assets keep exact binary identity through the
manifest, checksums, native smoke tests, and provenance attestations.

The old release-evidence contract, aggregate gate, and release-gate note tools
have no active owner and are removed. The reusable performance harness and its
workloads remain.

## Alternatives Considered

### Run a cheap preflight before the full gate

Rejected. It catches contract defects earlier but still makes publication wait
for more than one hour of noisy performance samples.

### Reduce the workload and sample counts

Rejected. A smaller performance gate is faster but still mixes observation
with release correctness and can still fail because of hosted-runner variance.

### Keep the gate and print better errors

Rejected. Better diagnostics are useful, but they do not correct the release
latency or the coupling between performance evidence and publication.

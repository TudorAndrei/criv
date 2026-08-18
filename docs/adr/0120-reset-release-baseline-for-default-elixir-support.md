---
id: ADR-0120
kind: decision
title: Reset the release baseline for default Elixir support
status: accepted
date: 2026-08-18
supersedes:
  - ADR-0118
governs:
  - Cargo.toml
  - Cargo.lock
  - .github/workflows/release.yml
  - scripts/performance/assemble-hosted-release-gates.sh
  - scripts/performance/src/bin/criv-discovery-gate.rs
  - scripts/performance/src/generate.rs
  - fixtures/performance/*.toml
  - tests/hosted_release_scripts.sh
---

# Reset the Release Baseline for Default Elixir Support

## Context

[[0118-bound-hosted-source-release-scaling|ADR-0118]] keeps the hosted release
rules that reject every normal dependency and stripped-binary size increase.
Those limits prove that the direct Source discovery replacement stays smaller
than the implementation it replaced. They do not leave room for a new default
language grammar.

[[0119-first-class-elixir-language-support|ADR-0119]] requires Elixir in the
default binary. The implementation needs the `tree-sitter-elixir` grammar and
the matching `ast-grep-language` feature. Making the parser optional would make
Elixir support dependent on how criv was built, which is not first class
support.

A preimplementation link measurement at commit `d9ad8c4` added one unique
normal package. On one macOS ARM host, the stripped release binary grew from
10,582,064 to 12,018,816 bytes. The two matched clean-build pairs were
84.91/85.30 seconds and 91.04/82.63 seconds. This measurement shows why the old
dependency and binary rules cannot accept the change. It is not acceptance
evidence for the completed production implementation.

## Decision

Keep every ADR-0118 release rule except the normal-dependency and binary-size
comparison for the first stable release that contains the complete ADR-0119
implementation.

Include Elixir support in the ordinary release binary. Permit exactly one new
unique normal package for this decision: `tree-sitter-elixir`. Do not permit a
second package, a new native compiler, bindgen, libclang, or a new native
library. The existing generated-parser build mechanism is not a new native
toolchain exception.

Treat the completed ADR-0119 implementation as a new compatible release
evidence contract. For its first stable release:

- build, strip, package, and verify the production binary on all four native
  release targets;
- record each candidate binary size and its delta from the prior stable
  baseline;
- allow the full binary-size increase that the completed production Elixir
  implementation needs, without a fixed byte or percentage limit;
- keep the clean-build median at no more than 110 percent of the matched prior
  baseline;
- keep the current memory, artifact identity, checksum, attestation, hosted
  platform, live-watch, and output-correctness rules; and
- publish no release when any selected `.ex` or `.exs` text file is skipped,
  sampled, truncated, or left unparsed because of its language.

After that release, use it as the compatible baseline. Restore the strict
no-growth binary comparison and the ordinary normal-dependency comparison for
later releases. A later dependency or binary increase needs its own accepted
decision.

Add deterministic `.ex` and `.exs` files to a mixed performance workload and
add a separate parse-heavy Elixir workload. Record elapsed time, memory, bytes,
selected file count, and output identity. The first production release records
the Elixir baseline. It has no Elixir-specific throughput threshold and no
comparison with Rust parsing speed. Later releases use the new production
baseline through the ordinary release performance rules.

The performance workload is evidence only. It cannot reduce correctness:
criv must read and parse every selected Elixir text file.

## Consequences

The first Elixir release can grow by the amount its complete production grammar
support needs. The receipt makes the increase visible on every target. Later
releases return to the strict growth rules with Elixir already in their
baseline.

The one named dependency exception is narrow. It does not permit unrelated
packages or weaken clean-build, memory, correctness, artifact, or hosted
platform gates.

The new workload gives later releases a stable Elixir comparison. It does not
turn parser speed into permission to omit supported files.

## Alternatives Considered

### Make Elixir an optional Cargo feature

Rejected. Users would not know whether an installed criv binary supports
Elixir, and policy behavior would depend on build flags.

### Keep a fixed binary-size increase limit

Rejected. The accepted behavior is the complete production language contract,
not the preimplementation measurement. The first release records the real
cross-platform cost and then becomes the new strict baseline.

### Add an Elixir-to-Rust throughput ratio

Rejected. criv runs as Rust and uses different grammars to parse different
source languages. Correctness requires all selected Elixir files to be parsed;
an arbitrary cross-language speed ratio does not define support.

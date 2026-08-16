---
id: ADR-0115
kind: decision
title: Build Source catalog and graph from one read
status: accepted
date: 2026-08-16
supersedes:
  - ADR-0112
  - ADR-0114
governs:
  - Cargo.toml
  - Cargo.lock
  - src/check.rs
  - src/config.rs
  - src/discovery/**/*.rs
  - src/lib.rs
  - src/refresh.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/state.rs
  - src/vault.rs
  - src/watch.rs
  - scripts/performance/discovery_probe.rs
  - scripts/performance/adapters/*.patch
  - scripts/performance/src/bin/criv-discovery-baseline.rs
  - scripts/performance/src/bin/criv-discovery-gate.rs
  - fixtures/performance/discovery/**/*
  - .github/workflows/ci.yml
  - .github/workflows/discovery-release-gates.yml
  - .github/workflows/release.yml
---

# Build Source Catalog and Graph From One Read

## Context

[[0112-direct-ignore-file-discovery|ADR-0112]] selects direct `ignore` after
the project removes `fff-search`, fuzzy ranking, frecency, and the second file
watcher. [[0114-reconcile-file-discovery-source-scopes|ADR-0114]] makes that
decision the active owner of file discovery.

The first direct implementation opens each Source candidate during discovery
to classify it as text or binary. Source graph construction opens each selected
file again to calculate its content hash and parse it. This duplicate read is
small in a sparse tree such as the observed Ouro repository. It becomes the
main cost in a dense Source root.

The original Source scaling gate compares unequal work. The `fff-search`
baseline reports paths after its extension-based classifier. The direct
`ignore` candidate opens every file to apply the approved extension-independent
binary contract. The 90,000-file results are about 2.98 seconds for the old
selector and 6.13 seconds for the first direct implementation. The accepted
1.19-second limit is not a valid limit for equal work.

Criv is a general-purpose tool. It must support large sparse trees, dense
Source roots, small repositories, and repeated live refreshes. One observed
repository or one synthetic shape must not own the architecture.

## Decision

Keep direct `ignore`, `globset`, `content_inspector`, and `notify`. Do not add
a custom walker, backend trait, runtime selector, fallback, native library,
zlob, or a criv fork of `ignore`. Do not restore `fff-search`, fuzzy ranking,
frecency, or its watcher.

Keep the Source, Vault, and Markdown profile behavior from the superseded
decisions. Keep their path identity, link and junction rejection, selected
read errors, duplicate removal, stable lexical order, target lookup, one-shot
and live parity, watch recovery, and the narrow native `ignore` control-file
error exception.

Split Source work into two internal phases:

1. Candidate discovery walks all configured roots once. It applies path,
   root, exclusion, link, junction, and metadata rules. It does not open file
   contents.
2. One Source build opens each candidate once. The same byte buffer supplies
   binary classification, the content hash, and parsing. Binary candidates do
   not enter the Source catalog or graph. The catalog is made from the graph's
   stable sorted paths, so both outputs have one selected set.

Hash the original bytes. When the binary contract classifies bytes as text but
the bytes are not valid UTF-8, parse replacement text without changing the
original-byte hash. Repository path identity must still be valid UTF-8 and
must never use lossy conversion.

A live Source event runs a new candidate walk and Source build. A docs-only
refresh reuses the last successful Source catalog and graph without a Source
walk or Source file read. A failed Source build keeps the last successful
published State and retries through the existing watch recovery rules.

Keep one Vault walk and the current Markdown full and changed selection. Keep
Source graph cache publication, Source target resolution, State output, editor
behavior, and the four-platform compatibility corpus.

## Performance Evidence

Measure Source candidate traversal and the complete Source build as separate
release-profile child processes. The complete Source build includes selection,
classification, original-byte hashing, and parsing. The immutable v0.9.0
adapter must run its real `fff-search` selector and its real Source graph build
for the matched complete-build row. Its candidate row runs only the old
selector. Production code must not contain measurement logic.

For both Source rows, the baseline and candidate must select the approved path
count and digest. At 9,000 selected files, candidate elapsed time is at most
110 percent of the matched baseline. At 90,000 and 225,000 selected files,
candidate elapsed time is at most 50 percent. Peak RSS is at most 110 percent
at all three sizes.

Keep the Source absolute elapsed and RSS limits from ADR-0112 for candidate
traversal. Do not apply those limits to the complete Source build because they
were measured for selection only. The equal-work baseline ratio is the hard
limit for the complete build.

Run both Source rows at all three sizes on the controlled primary host. Run
both 90,000-file rows on all four release platforms. Keep the accepted
five-sample stability rules. Keep the Ouro command, live convergence, Vault,
Markdown, artifact size, build time, toolchain, dependency, receipt, release,
and post-release rollback gates from ADR-0112.

## Consequences

Sparse trees keep the parallel pruning behavior of `ignore`. Dense Source
roots do not pay for two file opens before publication. Small repositories do
not start a second index or watcher. Markdown keeps the library that owns its
Git-ignore behavior.

Source selection completes during Source build, not during directory
traversal. A caller that needs selected Source paths must use the completed
Source build. Candidate paths are an internal traversal result and are not a
user contract.

The release gate now shows traversal cost separately from content work. It no
longer treats a different binary classifier as a traversal speed result.

## Alternatives Considered

### Select a backend from repository shape

Rejected. Two Source selectors can produce different results for hidden,
Git-ignored, linked, binary, and unreadable files. Verifying an accelerated
result with the authoritative selector removes most of the gain.

### Keep the duplicate classification read

Rejected. It is simple for sparse repositories but scales with every candidate
in dense Source roots. The graph already needs the complete contents.

### Classify from file extensions

Rejected. It changes the accepted binary contract and makes selection depend
on a file name rather than its bytes.

### Keep the old selector-only gate

Rejected. It compares different classification work and blocks an
implementation that preserves the accepted behavior.

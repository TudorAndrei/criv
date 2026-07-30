---
id: ADR-0047
kind: decision
title: Semantic Note Search Stays Source Only
status: accepted
date: 2026-07-30
supersedes:
  - ADR-0008
governs:
  - src/search.rs
  - Cargo.toml
---

# Semantic Note Search Stays Source Only

## Context

[[0008-optional-semantic-note-search|ADR-0008]] made semantic note search
optional so heavy embedding dependencies would not be forced into default
builds. It described the `fastembed` backend as "the local semantic backend".

Measurement on 2026-07-06 showed that characterization is wrong in a way that
matters. The enabled feature set in `Cargo.toml` selects `hf-hub-rustls-tls`,
which fetches model weights from the Hugging Face Hub at runtime, and
`ort-download-binaries-rustls-tls`, which downloads a prebuilt ONNX Runtime
shared library and loads it into the process. Neither is local.

The same measurement produced the numbers that settle the release question:

| Build | Binary size |
|-------|-------------|
| Default | 12,332,560 bytes |
| `--features embeddings` | 30,482,144 bytes |

A 147.2% increase runs against a release profile deliberately tuned for size.
First run also populates a 97 MB model cache under `.criv/embeddings`, and with
an empty cache and no network the failure is a hard error retrieving
`model.onnx`.

[[0001-local-cli-vault-architecture|ADR-0001]] warns explicitly against adding
network-dependent assumptions to a tool that runs inside git hooks and CI. An
embeddings-enabled release artifact would violate that on first use.

The feature is not dead: it compiles, passes its feature-gated tests, and
produces useful paraphrase matches on a small corpus.

## Decision

Keep semantic note search a source-only opt-in. `default = []` stays, and
release artifacts do not enable embeddings.

Document the real activation sequence, the first-run model download, the
`.criv/embeddings` cache, and the offline failure mode, rather than describing
the backend as local.

Revisit release artifacts only after large-vault benchmarks and a deliberate
distribution plan for the model and runtime dependency.

This supersedes ADR-0008. The optional-by-default decision is reaffirmed and
unchanged; what is corrected is the description of the backend as local, which
measurement disproved.

## Consequences

Default releases stay compact and deterministic, and `criv` keeps running
offline inside hooks and CI.

Semantic note search remains available to users who build from source and accept
a first run that reaches the network.

The project carries the optional dependency tree deliberately and documented,
instead of accidentally.

Two gates guard `--semantic`: the `embeddings` build feature and
`index.embeddings` in `criv.toml`. They are reported one at a time, and the flag
is visible in `criv search --help` on binaries that can never run it. Naming both
gates at once, or hiding the flag on non-embeddings builds, is tracked as a
follow-up in GitHub Issues.

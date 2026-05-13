---
id: ADR-0008
kind: decision
title: Optional Semantic Note Search
status: accepted
date: 2026-05-13
governs:
  - src/search.rs
  - src/config.rs
---

# Optional Semantic Note Search

## Context

The note retrieval layer needed more than substring search, but embedding
dependencies are heavy and should not be forced into every default build. The
spec named `fastembed` as the local semantic backend and `index.embeddings` as
the runtime opt-in.

## Decision

Keep lexical note search as the default path in [[src/search.rs]]. Add semantic
note search behind both a Cargo feature and the `index.embeddings = true`
configuration flag parsed by [[src/config.rs]].

When a user asks for semantic search without the feature or runtime flag, return
a clear error instead of silently falling back to lexical ranking.

## Consequences

Default criv builds remain lighter and deterministic, while users who want local
semantic retrieval can enable it deliberately.

Cargo still records optional dependency resolution in the lockfile. That is
acceptable because reproducibility matters more than keeping the lockfile small.

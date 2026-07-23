---
id: ADR-0042
kind: decision
title: Shared Source Index Lifecycle
status: accepted
date: 2026-07-23
governs:
  - src/source_index.rs
  - src/vault.rs
  - src/watch.rs
  - src/source_graph.rs
---

# Shared Source Index Lifecycle

## Context

[[0006-fff-source-index-and-incremental-watch|ADR-0006]] established fff as
the source-search and watcher backend, but it predated a single ownership model
for source enumeration, live watching, and graph construction. The vault could
enumerate source files independently of the fff-backed search index, while
watch rebuilds could construct fresh index state. That duplicated scanning and
made the source catalog used by graph construction vulnerable to drifting from
search, grep, partial-path resolution, and frecency behavior.

The old metadata-based graph cache identity also could reuse a parsed file after
its contents changed but its size and modification time were restored. A durable
graph cache and a live source watcher have different lifetimes and must not be
treated as the same cache.

## Decision

`src/source_index.rs#trait:SourceIndex` is the authoritative source-file
catalog for enabled source indexing. Its
`src/source_index.rs#type:FffSourceIndex` implementation owns enumeration,
fuzzy file search, grep, partial-path resolution, and source fingerprints;
vault graph construction consumes its `entries` rather than walking source
roots independently.

One-shot commands create one non-watching index per vault load. It may cache its
stable enumeration for that load. A long-running watcher creates one
watch-enabled fff index and shares it with every
`src/vault.rs#fn:load_incremental_with_source_index` rebuild. The live index
must not cache enumeration across watcher events, so add, modify, rename, and
delete events are visible to both graph rebuilding and search behavior.

`src/source_graph.rs#fn:build_incremental` remains the durable derived-data
cache. It identifies parsed files by BLAKE3 content digest and may hydrate from
or publish `.criv` graph-cache data independently of the live fff index.
`src/watch.rs#fn:rebuild_incremental` coordinates these two lifetimes: it
passes the shared live index into a vault rebuild while carrying the prior graph
and state only as incremental inputs.

## Consequences

Source search and source graph construction now observe one authoritative
catalog. The watcher avoids creating a second competing fff lifecycle, while a
one-shot command remains self-contained and deterministic.

Content hashing requires reading indexed source bytes, but it prevents stale
graph reuse when metadata collides. The cost is deliberate: correctness of the
durable graph cache takes priority over metadata-only reuse. Tests must cover
source roots, explicit file roots, excludes, ignored and hidden files, binary
filtering, duplicates, stable ordering, and live add/modify/rename/delete
visibility.

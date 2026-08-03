---
id: ADR-0066
kind: decision
title: Capability-Directed Wikilink And Query Resolution
status: accepted
date: 2026-08-03
governs:
  - src/vault.rs
  - src/query.rs
  - src/check.rs
  - src/state.rs
  - scripts/measure-performance.sh
---

# Capability-Directed Wikilink And Query Resolution

## Context

[[0020-portable-note-wikilinks|ADR-0020]] made file-backed Wikilinks the
portable representation of note references, and
[[0034-ast-aware-source-selectors|ADR-0034]] reserved Wikilinks for documents
while retaining bare source Wikilinks as a compatibility path. The protocol did
not define what happens when one bare target matches both a note and a source
basename. `Vault::resolve_link` in `src/vault.rs` currently makes source
resolution the first probe, so the accidental implementation order makes the
source win and sends every ordinary note or pattern link through the source
index.

The repeated source probe is visible in both validation and generated state.
`check::validate_links` in `src/check.rs` and `State::build_incremental` in
`src/state.rs` resolve the same parsed Wikilinks for different consumers, so a
source-like target can perform the same source work more than once in one vault
refresh.

Query dispatch has the same eager-data problem at a larger boundary.
`query::run` in `src/query.rs` loads a complete vault before matching the query
variant. Even `query diff`, which reads snapshots or Git state directly, and
queries that only inspect notes enumerate source files, build or hydrate the
source graph, and may publish `.criv/source-graph.json`. ADR-0042 deliberately
makes the source index authoritative when source facilities are enabled, but it
does not require commands that consume no source data to create those
facilities.

GitHub issues #41 and #6 require these observable precedence and loading
boundaries to be explicit before their common-case work can be removed.

## Decision

Dispatch Wikilinks by protocol and indexed meaning before attempting legacy
source compatibility:

1. A `source:` target resolves directly as a source target.
2. A `match:` target, including the note-qualified `#match:` form, resolves
   directly as an ADR policy pattern.
3. A target whose base resolves through the note ID, filename, or title indexes
   resolves as a note.
4. Only an otherwise-unclaimed bare target attempts legacy source resolution.

A bare note/source collision resolves as the note. Its fragment is a note
heading; if that heading is missing, the link is broken and does not fall
through to a same-named source symbol. Authors select the source side of a
collision explicitly with `source:`. Typed and unclaimed legacy source targets
retain their existing file ambiguity, symbol-fragment, warning, and broken-link
behavior.

Resolve every unique Wikilink target parsed from the notes once per loaded
vault and retain that result for validation, query, and generated-state
consumers. An ad hoc target may use the same dispatch without entering the
retained table. The table belongs to one `Vault` load and is rebuilt on every
one-shot command or watcher refresh; it is not a cache across refreshes.

Classify typed query variants by the data they consume before loading a vault:

- Snapshot-only queries, currently `diff`, do not load a vault.
- Docs-only queries load configuration, notes, headings, Wikilinks, policy
  metadata, and C4 artifacts, but do not read cached source state, construct a
  source index, enumerate or parse source files, build a source graph, or
  publish the source graph cache.
- Source-requiring queries use the complete vault path established by ADR-0042.

The docs-only set is `next-adr-id`, `cited-by`, `orphan-docs`, `nodes` when
restricted to documentation or decisions, and `c4-relationships`. Public
`cites` remains source-requiring because its existing result includes resolved
source and pattern targets. Unrestricted or code `nodes` and every query that
reads source paths, symbols, governance, coverage, C4 source anchors, or
generated C4 Code also remain source-requiring.

The load capability is fixed for one `Vault`; criv will not introduce a lazy
upgrade from docs-only to source-capable state. Existing full one-shot and
shared watcher loads, including `index.source = false`, keep their current
semantics.

## Consequences

Ordinary note and pattern links perform no source resolution. A collision that
previously resolved as a source now resolves as the note, which aligns the
compatibility fallback with the established document-link model and gives the
source interpretation an explicit spelling.

Validation and state generation observe one retained result for each unique
parsed target. The table consumes memory proportional to unique documentation
links but avoids repeated fuzzy source lookup and has no invalidation state
beyond the lifetime of the loaded vault.

Snapshot and docs-only queries become read-only with respect to source-derived
state. They do not pay source startup cost or contend on graph-cache
publication. Source-dependent query rows, ordering, diagnostics, source graph
cache format, and `criv.state.v0` remain unchanged.

Adding a query variant now requires choosing its capability in an exhaustive
dispatch. A wrong choice should fail focused tests that assert both result
compatibility and deterministic source-work counts.

Release measurements should compare the same binary profile, vault contents,
sample count, and machine conditions at three revisions: before either change,
after Wikilink dispatch/reuse, and after capability-directed query loading.
Deterministic work counters are the proof of removed internal work; repeated
whole-command timings are supporting evidence.

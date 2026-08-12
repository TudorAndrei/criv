---
id: ADR-0102
kind: decision
title: Request Local Query Reverse Indexes
status: accepted
date: 2026-08-12
governs:
  - src/query.rs
  - src/vault.rs
  - scripts/measure-performance.sh
  - scripts/performance/**
policy:
  patterns:
    - id: no-repeated-reverse-query-scans
      language: rust
      rule: |
        any:
          - pattern: cited_by($VAULT, $ID)
          - pattern: orphan_docs($VAULT)
          - pattern: references($VAULT, $SYMBOL)
          - pattern: nodes($VAULT, $KIND, $WITHOUT_DOCS)
      message: Reverse queries must use the request-local QueryReverseIndex instead of repeated vault scans.
targets:
  symbols:
    - src/query.rs#type:QueryReverseIndex
    - src/query.rs#type:QueryCommand/member:reverse_index_scope
    - src/query.rs#fn:collect_source_reference_keys
    - scripts/performance/src/main.rs#type:Case
---

# Request Local Query Reverse Indexes

## Context

`cited-by` scanned all notes and links for one requested note. `orphan-docs`
called that scan for each document. `references` scanned all note targets for
one requested source target. `nodes --kind code --without-docs` called that
scan for each source symbol. The last two loops made work increase with the
product of symbol count and note-reference count.

[[0066-capability-directed-wikilink-and-query-resolution|ADR-0066]] gives each
query one docs-only or source-capable `Vault`. Its retained Wikilink resolution
table removes repeated target resolution, but it does not remove repeated
iteration over notes. [[0072-keep-performance-observation-outside-core|ADR-0072]]
requires external release measurements and permits test-only work counts for
algorithmic correctness.

The approved `criv-medium` workload has 77 notes, 177 note links, 35 source
references, and 1,515 source symbols. Before this decision, five release
samples of `nodes --kind code --without-docs` had a median elapsed time of
26.787 seconds. This cost is large enough to justify one temporary index.

## Decision

Build at most one `QueryReverseIndex` for one `criv query` request. Build it
after the command loads its `Vault` and before query result construction. Drop
it when the command ends. Do not serialize it, put it in `.criv/`, retain it
between commands, or add invalidation behavior.

Only these commands build the index:

- `cited-by` and `orphan-docs` build the note-citation scope.
- `references` and `nodes --kind code --without-docs` build the source-reference
  scope.
- Other query commands build no reverse index.

The note-citation scope has these values:

- A resolved target note ID maps to the numeric positions of notes that cite
  it. A note contributes at most one position for one target ID. Self-citations
  do not enter this incoming map.
- One bit for each note records whether that note has any resolved outgoing
  note citation. A self-citation sets this bit.

The source-reference scope has these values:

- A canonical source path maps to the numeric positions of notes that refer to
  that file.
- A canonical source path plus its exact symbol fragment maps to the numeric
  positions of notes that refer to that symbol.
- A note contributes at most one position for one key. File queries use the
  path key. Symbol queries use the path-and-fragment key.

Store note positions in index values. Convert positions to display IDs only
when a query creates output rows. Keep the existing row sort order, duplicate
removal, unresolved-target behavior, self-citation behavior, text format, and
JSON format.

The construction cost is one linear pass over the selected input. The
note-citation scope visits each Wikilink once. The source-reference scope
visits each Wikilink and each frontmatter source target once. A lookup uses one
ordered-map lookup plus work proportional to its result rows.

Use these structural memory limits:

- The note scope stores one bit per note and no more than one incoming value
  row per distinct resolved, non-self `(citing note, target note)` pair.
- The source scope stores no more than two value rows per distinct resolved
  `(note, source target)` pair: one path row and, when present, one exact-symbol
  row.
- The index stores no note display ID in a value. It stores a numeric note
  position.

Correctness tests must prove the exact public rows for all four commands. A
test-only work count must prove that the source index resolves each input note
target once. Unit tests must also prove the value-row limits. These test-only
counts are not a runtime measurement interface.

Keep `query_orphan_docs` as the note-scope performance case. Add
`query_nodes_code_without_docs` as the source-scope case. The source-scope case
uses the approved `criv-medium` workload, an explicit release binary, five
successful samples, and the isolation and identity rules from ADR-0072.

The initial index is accepted only if the source-scope elapsed-time median is
at most 50% of the matched scan baseline. The measured indexed median was
0.097 seconds, or 0.36% of the 26.787-second baseline. The measured indexed
`query_orphan_docs` median was 0.027 seconds. A later change to the reverse
index must keep each matched medium-workload case at or below 110% of its
indexed baseline median. Machine-sensitive timing stays in the explicit
performance task and does not become a CI correctness gate.

Enforce the call boundary with
[[match:ADR-0102/no-repeated-reverse-query-scans]]. The policy rejects the old
free-function call forms that omitted the request-local index.

This decision refines ADR-0066 and ADR-0072. It does not supersede either
decision.

## Consequences

The two formerly repeated scans now pay one construction cost for one request.
The index uses temporary memory within the structural limits above. Query
results and persistent State do not change.

Every new reverse query must either use one of these two scopes or add a new
decision with measured data and an explicit memory limit. A persistent cache
needs a separate decision because it would add storage, invalidation, and
recovery contracts.

---
id: ADR-0184
kind: decision
title: Give Each Query Reverse Index Scope Its Own Type
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0169
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
      message: Reverse queries must use the request-local NoteCitationIndex or SourceReferenceIndex instead of repeated vault scans.
targets:
  symbols:
    - src/query.rs#type:NoteCitationIndex
    - src/query.rs#type:SourceReferenceIndex
    - src/query.rs#type:QueryCommand/member:answer
    - src/query.rs#fn:collect_source_reference_keys
    - scripts/performance/src/main.rs#type:Case
---

# Give Each Query Reverse Index Scope Its Own Type

## Context

[[0169-load-query-data-per-command|ADR-0169]] keeps the request-local reverse
index of [[0102-request-local-query-reverse-indexes|ADR-0102]]. One
`QueryReverseIndex` type held both scopes, and a scope value chose which
half to fill. A note-scope index carried an empty Source map, a Source-scope
index carried empty note maps, and each build checked the scope on every link.

## Decision

Retain the complete decision of ADR-0169, including the reverse-index
decision of ADR-0102 that it keeps, with one change: each scope is its own
type. `NoteCitationIndex` holds the notes that cite each note and the notes
with outgoing citations. `SourceReferenceIndex` holds the notes that reference
each Source file and symbol. A command arm builds the type it reads. There is
no scope value.

[[match:ADR-0184/no-repeated-reverse-query-scans]] keeps the call boundary of
ADR-0102.

## Consequences

An index holds only the rows of its scope. The query rows, their order, their
formats, and the work counts do not change.

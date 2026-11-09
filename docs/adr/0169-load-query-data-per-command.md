---
id: ADR-0169
kind: decision
title: Load Query Data per Command
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0102
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
    - src/query.rs#type:QueryCommand/member:answer
    - src/query.rs#fn:collect_source_reference_keys
    - scripts/performance/src/main.rs#type:Case
---

# Load Query Data per Command

## Context

[[0102-request-local-query-reverse-indexes|ADR-0102]] builds at most one
`QueryReverseIndex` for each query request and names the commands that build
each scope. Three code sites had to agree on each command: a capability table
chose the Vault load, a scope table chose the index, and the dispatch `match`
used both. `query diff` was caught before the dispatch, so the dispatch kept an
unreachable `diff` arm. The `cites` query took a `note_only` flag that every
caller set to false.

## Decision

Retain the complete reverse-index decision of ADR-0102: the scopes, the
commands that build them, the value-row limits, the work counts, and the
performance cases.

`QueryCommand::answer` is the one dispatch. Each command arm loads the data it
reads: the snapshot store or a Git ref for `diff`, a documentation-only Vault
for `next-adr-id`, `cited-by`, `orphan-docs`, and `nodes --kind doc|decision`,
and a Source-capable Vault for the other commands. An arm that needs a reverse
index builds its scope after the load. The `cites` query has no flag.

[[match:ADR-0169/no-repeated-reverse-query-scans]] keeps the call boundary of
ADR-0102.

The State projection and the query reverse index do not share one index.
Both read the Vault link resolution, but State publishes file-level `code:`
edges with partition dependencies, and the query index keys symbol fragments
by note position.

## Consequences

A new query command declares its data in one arm. The query rows, their order,
and their formats do not change.

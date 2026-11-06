---
id: ADR-0190
kind: decision
title: Judge Docs Watch Events by the Vault Prune Rules
status: accepted
date: 2026-11-06
governs:
  - src/discovery/mod.rs
  - src/watch.rs
---

# Judge Docs Watch Events by the Vault Prune Rules

## Context

[[0161-describe-source-languages-in-one-table|ADR-0161]] gave discovery the
`SourceScope` that both the Source walk and the Source event filter of the
watch read. Docs events had no such filter. The watch refreshed for every path
below the docs root, also below `target`, `node_modules`, `.git`, and `.criv`,
which the vault walk prunes. A build output inside the docs root therefore
caused refreshes that could not change the vault.

The watch also listed its configured locations three times: once for the
active watch set, once for the recovery watch set, and once for the topology
that detects a changed root.

## Decision

`src/discovery/mod.rs` owns `VaultEventFilter` beside `SourceEventFilter`. A
docs event is relevant when its path is below the docs root and no component
is a directory that the vault walk prunes. The vault walk and the filter use
the same prune rules.

The watch derives its configured locations once: the docs root and each Source
root. The active watch set, the recovery watch set, and the topology read that
one list.

## Consequences

A change below a pruned directory of the docs root does not refresh State.
A new kind of watched location is added in one place.

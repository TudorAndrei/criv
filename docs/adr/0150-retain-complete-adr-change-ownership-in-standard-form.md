---
id: ADR-0150
kind: decision
title: Retain Complete ADR Change Ownership in Standard Form
status: accepted
date: 2026-10-30
supersedes:
  - ADR-0140
governs:
  - src/adr.rs
  - src/enforce.rs
---

# Retain Complete ADR Change Ownership in Standard Form

## Context

[[0140-own-the-complete-adr-change-decision|ADR-0140]] is an accepted narrative
without the sections required by
[[0148-separate-markdown-formatting-from-vault-validation|ADR-0148]]. Preserve
its historical bytes and carry its complete ownership decision forward here.

## Decision

The ADR module owns the complete change-permission decision. Its interface
takes RepositoryFiles, Config, an optional ChangedSet, and a typed commit,
push, or CI mode. It returns violation strings in enforcement order. Receipt
flags and permission callbacks do not cross that interface.

Enforcement retains comparison selection, environment reads, pre-push input,
policy scans, import checks, failure priority, report formatting, and the CI
check-base preflight before selected-change validation.

Retain ADR-0012 immutability, receipt proof order, stage exceptions,
branch-local CI changes, mechanical Wikilink migration, scope matching,
violation order, and failure text. Repository-handle reads follow ADR-0138 and
fail closed. Git reference reads stay in the Git interface.

Identity proof remains in the ADR owner. Source receipt and history proof stay
in its private reconciliation child; snapshot capture and rollback stay in the
transaction child. Reconciliation commands verify staged proof before commit.

## Consequences

The active record meets the required format. Module boundaries, receipt
schemas, enforcement behavior, and existing tests remain unchanged.

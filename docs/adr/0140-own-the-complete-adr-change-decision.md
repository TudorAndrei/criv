---
id: ADR-0140
kind: decision
title: Own the Complete ADR Change Decision
status: accepted
date: 2026-09-05
governs:
  - src/adr.rs
  - src/enforce.rs
---

# Own the complete ADR change decision

Enforcement assembled receipt flags, per-file exceptions, and decision scope
checks. A caller thus had to know the ADR permission rules to use the ADR
module. Put the complete change decision in `src/adr.rs`. Its interface takes
`RepositoryFiles`, `Config`, an optional `ChangedSet`, and a typed commit, push,
or CI mode. It returns violation strings in enforcement order. No receipt
flags or permission callback cross this interface.

Keep comparison selection, environment reads, pre-push input, policy scans,
import checks, failure priority, and report formatting in `src/enforce.rs`.
Keep the CI `check_base` preflight before selected-change validation.

Keep the rules from [[0012-adr-immutability-enforcement|ADR-0012]]. Preserve
receipt proof order, stage exceptions, branch-local CI changes, mechanical
Wikilink migration, scope matching, violation order, and failure text.
Working-tree proof reads use the supplied repository handle under
[[0138-confine-repository-reads|ADR-0138]]. Failed reads cannot prove permission.
Git reference reads continue through the Git interface.

Extend the owner interface from
[[0123-own-reconciliation-under-the-adr-module|ADR-0123]]. Identity proof stays
in the ADR module. Source receipt and history proof stay in its private source
reconciliation child. Snapshot capture and rollback stay in the transaction
child. Enforcement no longer needs the separate receipt questions. The
reconciliation commands still check their staged proof before commit.

This changes ownership, not the receipt schemas or enforcement contract. It
keeps the repository and module boundaries from
[[0139-name-the-glob-module-for-what-it-holds|ADR-0139]]. Keeping permission
assembly in Enforcement would leave the caller responsible for ADR rules.

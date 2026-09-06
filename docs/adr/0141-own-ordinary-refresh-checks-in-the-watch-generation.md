---
id: ADR-0141
kind: decision
title: Own Ordinary Refresh Checks in the Watch Generation
status: accepted
date: 2026-09-05
governs:
  - src/watch.rs
  - src/refresh.rs
---

# Own ordinary refresh checks in the Watch generation

`LiveWatchSession` assembled the precommit configuration check and the second
comparison after each ordinary refresh. The active generation already held the
configuration and refresh state. Put the complete ordinary refresh operation
in `ActiveWatchGeneration`. It takes the repository handle and refresh cause,
and returns an owned summary, failure, or reconfiguration request.

Keep both configuration comparisons. The precommit check rejects a changed or
unreadable configuration at the existing State publication callback. After the
attempt, compare the configuration again. Reconfiguration takes priority over
the refresh result. The session keeps event polling, suspension, recovery
watches, retry timing, failure reports, and generation replacement.

Keep the candidate rule from
[[0092-transactional-live-watch-generations|ADR-0092]]. A candidate uses one
configuration snapshot and an initial `RefreshSession::refresh`. Do not apply
the ordinary refresh guard to candidate creation. Configuration events during
candidate work remain queued for the next observation.

Keep Source reuse and failed Source refresh retry in `RefreshSession`, under
[[0126-own-one-source-state-per-refresh|ADR-0126]]. Keep the disk transaction,
rollback, and callback position under
[[0094-automatic-recoverable-state-publication|ADR-0094]]. Keep the lock order
from [[0139-name-the-glob-module-for-what-it-holds|ADR-0139]]. A rejected
publication preserves the last successful State and refresh result.

Deterministic tests use a test-only checkpoint held by one refresh session.
The checkpoint runs at the existing precommit callback. It introduces no
production callback interface or process-global test state.

Keeping these checks in the event loop would require the caller to assemble
the generation rules. This move keeps those rules with the configuration they
protect. It does not change candidate acceptance, one-shot refresh, State
schemas, publication order, retry intervals, or user-facing reports.

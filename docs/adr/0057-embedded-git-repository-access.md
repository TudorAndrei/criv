---
id: ADR-0057
kind: decision
title: Embedded Git Repository Access
status: accepted
date: 2026-08-02
governs:
  - Cargo.toml
  - src/git.rs
  - src/query.rs
  - src/enforce.rs
policy:
  patterns:
    - id: no-runtime-git-subprocess
      language: rust
      rule: |
        all:
          - pattern: Command::new("git")
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: Runtime repository reads must use the embedded Git boundary; Git executable calls belong only in the test module fixture helpers.
---

# Embedded Git Repository Access

## Context

criv previously read Git state by launching the `git` executable from query and
enforcement paths. That made repository behavior depend on PATH and process
environment even though the CLI already receives an explicit vault root. It also
made ref lookup, diff parsing, pre-push traversal, and content reads separate
subprocess protocols rather than one bounded repository API.

[[0007-content-addressed-state-and-diffing|ADR-0007]] requires `query diff` to
resolve a Git ref when no local snapshot matches. The audit finding in
[[0048-2026-07-25-audit-findings-not-actioned|ADR-0048]] found the former
`git show` invocation safe from shell injection, but it did not make the
executable dependency necessary. Accepted decisions are immutable under
[[0012-adr-immutability-enforcement|ADR-0012]], so this decision records the
replacement without altering either earlier ADR.

## Decision

Use direct `git2 v0.21.0` with default features disabled as criv's embedded,
local-repository backend. `src/git.rs` is the only production boundary that
uses `git2`; it discovers from the explicit root, resolves refs, reads index and
tree blobs, calculates changed sets, finds merge bases, and traverses outgoing
commits. It returns criv-owned values and `CrivError` diagnostics rather than
leaking dependency objects to callers.

`src/query.rs#fn:load_git_state` uses that boundary for `.criv/state.json` ref
lookup. `src/enforce.rs#fn:changed_entries`,
`src/enforce.rs#fn:pre_push_changed_entries`, and
`src/enforce.rs#fn:read_changed_content` use it for staged, worktree, CI,
manual-push, and pre-push behavior. The contract preserves explicit-root
discovery, local-snapshot precedence, comparison basis text, best-effort content
reads, UTF-8 rejection, and the documented comparison fallbacks.

The runtime performs no Git transport operation and has no executable fallback.
`Command::new("git")` is prohibited in governed production code by
[[match:ADR-0057/no-runtime-git-subprocess]]. The structural rule excludes only
the `mod tests` fixture/oracle helpers in `src/enforce.rs`;
integration tests outside the governed runtime paths may also build fixture
repositories with the Git CLI. The
`tests/runtime_git_subprocess_guard.rs` integration test additionally checks the
production portions of each runtime module, including `src/enforce.rs`.

## Consequences

`criv query diff` and `criv enforce` continue to work when no usable `git`
executable is on PATH. The test suite deliberately retains Git CLI fixture and
differential-oracle helpers so that embedded behavior remains comparable to
Git's observable behavior.

The dependency graph now has direct `git2 v0.21.0` as well as `git2 v0.20.4`
transitively through `fff-search`. Both resolve to the same `libgit2-sys`
release, but the older wrapper's monitor-only advisories remain open until its
upstream path changes. Artifact and audit evidence is maintained in
[[dependency-evaluations]]; this decision neither replaces `fff-search` nor
claims those transitive advisories are fixed.

The release artifact grew by 34,720 bytes (0.27%) in the recorded local
same-toolchain measurement. The clean embedded build was slower, which is a
release-review cost rather than a behavior change or a blocker. Future changes
to repository access must remain local-only unless a new ADR explicitly admits
transport behavior.

This extends ADR-0007's ref-resolution implementation, preserves ADR-0012's
append-only governance, and narrows the operational implication of ADR-0048's
former `git show` finding without superseding that multi-topic audit decision.

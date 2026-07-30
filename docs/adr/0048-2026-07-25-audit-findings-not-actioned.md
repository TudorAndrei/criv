---
id: ADR-0048
kind: decision
title: 2026-07-25 Audit Findings Not Actioned
status: accepted
date: 2026-07-30
---

# 2026-07-25 Audit Findings Not Actioned

## Context

The read-only audit of 2026-07-25 at commit `d549a2b` examined several candidate
findings and concluded that none of them was actionable. That reasoning lived in
a "Considered And Rejected" section of `ISSUES.md`.

Open findings from that audit have since moved to GitHub Issues, and `ISSUES.md`
was removed. A decision not to act is still a decision, and under the repository
convention it belongs in an ADR rather than in a deleted file. Without this
record, a later audit re-derives the same conclusions from scratch, or worse,
reverses one of them without knowing it was examined.

## Decision

Record the following as examined and deliberately not actioned. Reopening any of
them requires new evidence, not a re-reading of the same code.

**Transitive crate-version duplicates.** rand, phf, and indexmap duplicates all
originate in independent upstreams criv does not control. Not actionable.

**`fff-search` transitive `git2` and `bincode` advisories.** These have a
documented monitor-only posture in `docs/dependency-evaluations.md`. `cargo audit
--no-fetch` matched the recorded snapshot exactly, so the posture stands.

**Mermaid SVG injection.** Insertion in both editors is guarded by
`securityLevel: "strict"`. Re-checked; no bypass found.

**`git show` argument handling.** `query` and `enforce` pass arguments directly
to `Command` with no shell. Re-checked; fine.

**Obsidian DOT sanitizer `<style>` gap.** This remains an open investigation
rather than a finding. The audit looked specifically for evidence that Graphviz
output can carry attacker-influenced `<style>` from `.c4` source and found none;
the `stylesheet` graph attribute emits an `<?xml-stylesheet?>` processing
instruction, which `.obsidian/plugins/criv/src/core.ts:262` strips.

**Path confinement on generated writes.** Complete. `src/state.rs:418`,
`src/source_graph.rs:165`, `src/check.rs:257`, and `src/architecture.rs:23` all
route through `write_atomic_in`, and `prepare_confined_write` re-checks for
symlink components after `create_dir_all`, closing the TOCTOU window. The
separate init path was brought under the same rule during the same remediation
round.

**ReDoS through inline ADR policy patterns.** Inline `policy.patterns` compile
through ast-grep, whose `regex:` support is backed by the linear-time `regex`
crate, so the angle does not apply. Glob scoping surfaces compile errors as
errors rather than matching everything.

**CI supply chain.** Both workflows set top-level `permissions: contents: read`,
pin every action to a full SHA, and use `persist-credentials: false`. There is no
`pull_request_target`, and the release job verifies the tag is an ancestor of
`origin/main` before publishing. Nothing to report.

**`@types/vscode` version drift.** Resolving to 1.125.0 against a 1.85.0 pin is a
local `npm install` artifact in the working tree, not committed drift. CI's
`npm ci` installs 1.85.0.

**DOT generation escaping.** `src/c4_code.rs:109-123` handles backslash, quote,
newline, and tab and drops carriage returns, so repository-controlled symbol
names cannot break out of the generated DOT string.

## Consequences

A later audit that surfaces one of these can check this ADR first and either
supply the new evidence that reopens it or move on.

The audit's scope limits are recorded with the same intent. It was
standard-depth and hotspot-weighted, and did not cover manual Obsidian or VS Code
UI behavior, GitHub Actions supply-chain posture beyond the existing zizmor gate,
or runtime profiling. Its performance findings are read-derived; the two that
survive as GitHub issues both call for before-and-after measurement with
`mise run perf` rather than treating the reading as proof.

The baseline the audit verified against was `npm audit` on both packages, which
found one high advisory in the VSIX packaging path and none in the Obsidian
plugin, and `cargo audit --no-fetch` matching the snapshot in
`docs/dependency-evaluations.md` byte-for-byte.

An LSP server was considered and rejected on the same pass. It would collapse the
duplicated TypeScript diagnostics, completion, hover, and definition logic into
one Rust implementation, but it adds a long-lived server process to a project
whose ADR-0001 is deliberately one-shot-CLI-shaped, adds a JSON-RPC dependency to
the curated set ADR-0003 governs, and would not replace the webview previews. The
editor duplication it would have addressed is tracked as its own issue instead.

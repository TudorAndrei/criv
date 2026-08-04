---
name: criv-me
description: Use when the user wants to develop a plan, settle an architectural or product decision, or stress-test a proposal against the existing criv vault before writing anything down.
metadata:
  criv-template: blake3:fd67879291fced0d
---

# criv-me

criv-me is an interview: settle one decision at a time with the user, then
write the settled rationale into the vault.

## Ground the session in the vault

The existing documents, ADRs, wiki-links, governed scopes, and source code are
the decision context. Find the relevant ones first with `rg`, `criv query`, or
a direct read from `docs/`. Read the source whenever it answers a factual
question, and spend the user's attention only on intent, tradeoffs,
constraints, and choices the repository cannot settle.

The vault already carries the project language, rationale, and governance.
Take those from the vault rather than from conventions of other workflows.

## Interview

Ask one question at a time, and give your recommended answer with its
reasoning inside the question.

Walk the dependencies in order: terms and constraints, then irreversible
architecture, then implementation boundaries, enforcement, tests, rollout, and
documentation.

Challenge a fuzzy or overloaded term by proposing a precise project term.
Challenge a claim that conflicts with the code, a document, an ADR, or a
governed scope. Use a concrete scenario or an edge case to expose an unclear
boundary.

This part is done when every settled answer is stated in the user's own
accepted words, and each open question has an owner or a stated assumption.

## Capture the outcome

Write a settled explanation into the document it belongs to. Reach for a new
ADR only when the decision is hard to reverse, surprising without its context,
and the result of a real tradeoff; otherwise update or supersede the ADR that
already covers the ground.

The `writing-decisions` skill owns the ADR format, governs scopes, and policy
patterns. The `referencing-code` skill owns the wiki-link forms that connect
the decision to source.

The session is complete when every settled decision is written down and the
vault is green, as the `checking-drift` skill defines it.

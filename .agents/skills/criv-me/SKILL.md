---
name: criv-me
description: Use when the user wants to develop a plan, make architectural or product decisions, stress-test a proposal against existing criv docs/ADRs/code, and capture settled rationale in the criv documentation graph.
---

# criv-me

Use `criv-me` as a decision-development mode for criv vaults.

## Grounding

- Treat existing criv docs, ADRs, wiki-links, governed scopes, and source code as the decision context.
- Start by finding relevant docs and ADRs with `rg`, `criv query`, or direct reads from `docs/`.
- Inspect source code when it can answer a factual question. Ask the user only for intent, tradeoffs, constraints, or choices that the repo cannot determine.
- Do not import `CONTEXT.md` conventions from other workflows. In criv, docs and ADRs already carry project language, rationale, and governance.

## Session style

- Interview the user one decision at a time.
- For each question, give your recommended answer and the reasoning behind it.
- Walk dependencies in order: clarify terms and constraints before irreversible architecture, then implementation boundaries, enforcement, tests, rollout, and documentation.
- Challenge fuzzy or overloaded terms by proposing a precise project term.
- Challenge claims that conflict with code, existing docs, ADRs, or governed scopes.
- Use concrete scenarios and edge cases to expose unclear boundaries.

## Capturing outcomes

- Update criv docs inline when a settled explanation should persist.
- Create or update an ADR only when the decision is hard to reverse, surprising without context, and the result of a real tradeoff.
- Use the existing criv ADR format under `docs/adr/`: `id`, `kind: decision`, `title`, `status`, `date`, and relevant `governs:` scopes.
- When a decision includes an enforceable structural rule, add an inline `policy.patterns` entry with `id`, `language`, and either `pattern` or `rule`.
- Never put ADR-owned structural rules in `criv.toml` as `[patterns."ADR-NNNN/..."]`; keep the rule body in the owning ADR's `policy.patterns` frontmatter.
- Link decisions and docs to source with criv wiki-links such as `[[src/lib.rs#run]]`, `[[src/lib.rs#L10-L20]]`, `[[match:ADR-0007/pattern-id]]`, and `[[0007-content-addressed-state-and-diffing|ADR-0007]]`.
- Prefer updating or superseding an existing ADR over creating a duplicate decision note.

## Validation

- Run `criv watch --once` after docs, ADR, or code changes to refresh `.criv/state.json`.
- Run `criv check` before declaring documentation work complete.
- Run `criv search --rule ADR-NNNN` to inspect inline policy matches when adding or changing ADR policy patterns.
- Run `criv enforce --stage ci` when the session changes ADR-governed code or policy patterns.

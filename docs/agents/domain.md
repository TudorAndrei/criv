---
id: agents-domain
kind: doc
title: Domain Docs For Agent Skills
---

# Domain Docs

How the engineering skills should consume this repo's domain documentation when
exploring the codebase. This is a single-context repo.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root, if it exists.
- **`docs/adr/`** — read ADRs that touch the area you are about to work in.
  There are 47 of them and they are the primary record of how criv behaves and
  why.
- **`AGENTS.md`** at the repo root — verification commands and repo conventions.

If `CONTEXT.md` doesn't exist, **proceed silently**. Don't flag its absence and
don't suggest creating it upfront. The `/domain-modeling` skill creates it lazily
when terms actually get resolved.

## File structure

```text
/
├── AGENTS.md
├── docs/
│   ├── adr/                ← 47 decisions, criv-governed
│   ├── agents/             ← this configuration
│   ├── architecture/       ← agent-authored LikeC4 workspace
│   └── query-reference.md
└── src/
```

## Finding the right ADR

`docs/adr/` is large enough that reading it linearly is the wrong move. Use criv:

```sh
criv query governing src/watch.rs      # which ADRs govern this file
criv query governs ADR-0007            # what an ADR controls
rg -n -i "<topic>" docs/               # search note text
```

## ADR conventions specific to this repo

- Accepted ADRs are **immutable** under ADR-0012. Never edit, delete, or rename
  one to change a decision. Write a new ADR with `supersedes:` instead.
- Get the next number from `criv query next-adr-id`, never by guessing.
- ADRs carry `governs:` path globs, and may carry inline `policy.patterns`
  ast-grep rules that `criv check` and `criv enforce` evaluate.
- Everything under `docs/` is a criv vault note and needs frontmatter with at
  least `id`, `kind`, and `title`, or `criv check` fails and blocks the commit.

## Use the project's vocabulary

When your output names a domain concept — an issue title, a refactor proposal, a
hypothesis, a test name — use the term the ADRs already use. Don't drift to
synonyms.

If the concept you need isn't named anywhere yet, that's a signal: either you're
inventing language the project doesn't use (reconsider) or there's a real gap
(note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than
silently overriding:

> _Contradicts ADR-0007 (content-addressed state) — but worth reopening because…_

Because accepted ADRs are immutable here, "reopening" concretely means writing a
superseding ADR, not amending the old one.

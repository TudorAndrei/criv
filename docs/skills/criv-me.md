---
id: criv-me
kind: doc
title: Criv Me
tags: [criv, skill, decisions]
---

# Criv Me

Use `criv-me` to develop plans and decisions against the existing criv vault.

Core workflow:

- Read relevant docs, ADRs, and code before accepting a premise.
- Ask one decision question at a time, and include the recommended answer.
- If code or criv state can answer the question, inspect that instead of asking.
- Challenge ambiguous terms, hidden constraints, ADR conflicts, and mismatches between code and docs.
- Capture settled durable decisions in criv ADRs; capture ordinary explanation in `kind: doc` notes.
- Use criv wiki-links when referencing source files, symbols, patterns, docs, and ADRs.
- Run `criv watch --once` and `criv check` after documentation changes.

Do not import `CONTEXT.md` conventions from non-criv workflows. In criv, the docs
and ADR graph is the source of project language, rationale, and governance.

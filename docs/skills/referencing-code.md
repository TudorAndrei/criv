---
id: referencing-code
kind: doc
title: Referencing code
tags: [criv, skill]
---

# Referencing code

Use wiki-links for code, pattern, and note references.

- Source file: `[[src/auth/verify.rs]]`
- Source symbol: `[[src/auth/verify.rs#verify_token]]`
- Source lines: `[[src/auth/verify.rs#L42-L67]]`
- Pattern: `[[match:ADR-0007/no-block-on-in-handler]]`
- Note: `[[ADR-0007]]`

Partial source paths are allowed, but `criv check` warns when they are ambiguous.

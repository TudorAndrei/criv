---
name: referencing-code
description: Use when adding criv wiki-links from docs or ADRs to source files, symbols, line ranges, patterns, and notes.
---

# Referencing code

Use wiki-links for code, pattern, and note references.

- Source file: `[[src/auth/verify.rs]]`
- Source symbol: `[[src/auth/verify.rs#verify_token]]`
- Source lines: `[[src/auth/verify.rs#L42-L67]]`
- Pattern: `[[match:ADR-0007/no-block-on-in-handler]]`
- Note: `[[0007-content-addressed-state-and-diffing|ADR-0007]]`

Partial source paths are allowed, but `criv check` warns when they are ambiguous.

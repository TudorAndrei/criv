---
name: referencing-code
description: Use when writing a criv wiki-link from a document or ADR to a source file, symbol, line range, policy match, or another note.
---

# Referencing code

A wiki-link is how a document claims a piece of the repository. criv resolves
each one and reports drift when the target moves.

| Target | Form |
| --- | --- |
| Source file | `[[src/auth/verify.rs]]` |
| Source symbol | `[[src/auth/verify.rs#verify_token]]` |
| Source lines | `[[src/auth/verify.rs#L42-L67]]` |
| Policy match | `[[match:ADR-0007/no-block-on-in-handler]]` |
| Another note | `[[0007-content-addressed-state-and-diffing\|ADR-0007]]` |

Prefer a symbol over a line range. A symbol survives edits above it; a line
range does not.

A partial source path resolves while it stays unique. Write the path from the
repository root when `criv check` warns that a path is ambiguous.

The links are correct when the vault is green after `criv watch --once`.

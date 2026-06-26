---
id: ADR-0040
kind: decision
title: Inline Only ADR Policy Rules
status: accepted
date: 2026-06-26
supersedes:
  - ADR-0039
governs:
  - src/vault.rs
  - src/check.rs
  - src/enforce.rs
  - src/structural.rs
  - src/search.rs
  - src/state.rs
  - README.md
---

# Inline Only ADR Policy Rules

## Context

[[0039-inline-adr-policy-rules|ADR-0039]] moved ADR policy enforcement away
from generated config and into inline `policy.patterns` definitions. It kept a
compatibility path for ID-only ADR policy entries that resolved through
`criv.toml` or fell back to treating the ID as a raw ast-grep pattern.

That compatibility path now has more cost than value. It splits ADR enforcement
between ADRs and config, makes invalid policy entries silently inert or
surprising, and keeps unused fallback code in the scanner.

## Decision

ADR policy entries must be inline definitions. Each `policy.patterns` item must
declare `id`, `language`, and exactly one of `pattern` or `rule`.

Remove ID-only ADR policy compatibility. `criv check`, `criv search --rule`,
`criv enforce`, and state generation should use only the inline policy
definition stored in the ADR.

Keep standalone configured `[patterns.*]` entries for explicit
`criv search --pattern-id` usage and pattern wikilinks. They are no longer a
policy body source for ADR enforcement.

## Consequences

Accepted ADRs become the single source for policy enforcement behavior. Policy
misconfiguration is visible as a validation error instead of being hidden behind
config fallback behavior.

Repos with ID-only ADR policies must migrate those policy entries by moving the
ast-grep body into the ADR frontmatter.

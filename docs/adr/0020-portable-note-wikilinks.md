---
id: ADR-0020
kind: decision
title: Portable Note Wikilinks
status: accepted
date: 2026-06-13
governs:
  - src/check.rs
  - src/vault.rs
  - src/enforce.rs
  - src/init/templates.rs
---

# Portable Note Wikilinks

## Context

[[0002-docs-and-adrs-form-the-governance-graph|ADR-0002]] established
wiki-links as the connective tissue for criv documentation. criv resolves note
links through metadata such as frontmatter `id`, filename stem, and title, so a
link like `[[ADR-0010]]` is valid to criv when a note has `id: ADR-0010`.

That metadata-only target is not portable. Obsidian and other Markdown editors
that resolve wiki-links by file name can treat `[[ADR-0010]]` as a missing file
even though `docs/adr/0010-criv-init-installs-agent-runtime-skills.md` exists.
The Obsidian companion behavior from
[[0009-obsidian-plugin-as-state-consumer|ADR-0009]] can improve vault
ergonomics, but ordinary note navigation should not depend on a criv-specific
plugin.

[[0012-adr-immutability-enforcement|ADR-0012]] also means the repository needs a
controlled migration path before accepted ADR bodies can be retargeted from
metadata-only note links to file-backed note links.

## Decision

Use file-backed wiki-link targets for note references. When the visible label
should remain a stable ADR ID, use a wiki-link alias:

```markdown
[[0010-criv-init-installs-agent-runtime-skills|ADR-0010]]
```

The canonical target for an ADR note is its Markdown filename stem. A
repo-relative Markdown path is also acceptable when a same-stem ambiguity needs
to be avoided. The displayed label may remain the ADR ID.

`criv check` should continue to understand metadata-only note links for
backward-compatible resolution, but it should report them as non-portable note
links when they do not name an actual note file stem or path. Pattern links such
as `[[match:ADR-0007/no-block-on]]` and
`[[ADR-0007#match:ADR-0007/no-block-on]]` remain pattern references, not note
references.

`criv enforce` should permit accepted ADR body edits only for mechanical
wiki-link portability migrations that preserve the resolved note or pattern
semantics. It must continue to reject ADR renames, deletions, and decision text
rewrites.

Docs, ADRs, runtime skill templates under `assets/skills/**`, installed skill
copies under `.agents/skills/**`, and generated Obsidian plugin fixtures should
use the portable note-link convention.

## Consequences

Markdown editors that understand plain file-stem wiki-links can open ADR
references without installing the criv Obsidian plugin or reading criv state.

criv keeps metadata resolution for compatibility and query behavior, but
validation makes new metadata-only note links fail fast with an actionable
diagnostic.

The migration requires a narrow exception to ADR immutability enforcement. That
exception is intentionally limited to link target changes whose resolved meaning
is unchanged.

---
id: ADR-0033
kind: decision
title: Typed Wikilink Source References
status: accepted
date: 2026-06-21
governs:
  - src/check.rs
  - src/vault.rs
  - .obsidian/plugins/criv/src/main.ts
---

# Typed Wikilink Source References

## Context

[[0020-portable-note-wikilinks|ADR-0020]] made note references portable to
Obsidian and other Markdown editors by requiring note wiki-links to target real
Markdown file stems or paths. That decision fixed metadata-only note links such
as `[[ADR-0010]]`, but it did not decide how prose should link to source files.

criv currently accepts source wiki-links such as `[[src/check.rs]]` and
`[[src/vault.rs#resolve_source_target]]` because the vault graph resolves them
through the source index and source graph. Obsidian's default internal link
format is also Wikilinks, but Obsidian resolves those links as note or vault-file
targets. In practice, Obsidian can mark criv source links such as
`[[src/structural.rs]]` as links to non-existent documents even when `criv
check` passes.

Switching prose source references to Markdown links would make Obsidian quieter,
but it would create a second source-reference syntax beside criv's existing
wiki-link graph. It would also make source references less obviously typed in
the same way as pattern links such as `[[match:ADR-0007/no-block-on]]`.

## Decision

Keep Wikilinks as the canonical note-prose link syntax for criv documentation.

Use explicit typed Wikilink targets for non-note criv references. Source
references should use the `source:` target type:

```markdown
[[source:src/check.rs]]
[[source:src/vault.rs#resolve_source_target]]
[[source:src/check.rs#L822-L865]]
```

Pattern references continue to use `match:` targets. Note references continue to
use file-backed note Wikilinks from [[0020-portable-note-wikilinks|ADR-0020]].

`criv check` should continue to resolve legacy bare source wiki-links for
compatibility, but it should report them as non-portable source links and
suggest the typed `source:` form. The check should not suggest Markdown links as
the canonical fix.

The Obsidian companion plugin should treat typed source Wikilinks as criv-owned
references. It may decorate, preview, autocomplete, and suppress false missing
document feedback for them, but the CLI remains the authority for whether the
source target resolves.

## Consequences

The documentation graph keeps one primary link family: Wikilinks. That matches
Obsidian's default authoring format while making criv-specific non-note
references explicit.

Typed source links are not plain Obsidian note links. Without criv plugin
support, Obsidian may still treat them as missing documents. That is acceptable:
the target type is intentionally criv-specific, and `criv check` plus the plugin
own its semantics.

Existing bare source wiki-links need a compatibility window. They should keep
resolving through criv, but validation should make the migration path visible so
authors converge on typed source references.

---
id: ADR-0050
kind: decision
title: Repeated Wikilinks Are Not A Defect
status: accepted
date: 2026-07-30
governs:
  - src/check.rs
---

# Repeated Wikilinks Are Not A Defect

## Context

The 2026-07-25 audit found `docs/tooling.md` ending with the same wiki-link on
two consecutive lines, and recorded it as evidence that "the doc gate does not
catch repeated wiki-links" — implying `criv check` should grow a diagnostic for a
wiki-link appearing more than once in a note.

Measured against this vault, that rule would be wrong far more often than right.
Six of the 39 notes containing wiki-links — 15% — repeat one, and the repeats are
correct writing rather than accidents.

[[0015-size-optimized-release-profile|ADR-0015]] is the clearest case. It cites
[[0014-tag-triggered-release-binary-workflow|ADR-0014]] in its Context, to
establish what the release workflow is, and again in its Consequences, to state
that the workflow is unchanged. Each section stands alone and links what it
refers to, which is what makes an ADR readable by section rather than only
end-to-end. The same shape appears in
[[0011-embed-runtime-skill-templates-as-assets|ADR-0011]],
[[0020-portable-note-wikilinks|ADR-0020]],
[[0023-do-not-track-generated-plugin-bundles|ADR-0023]], and
[[0033-typed-wikilink-source-references|ADR-0033]].

The finding generalized from a single instance, and in the wrong direction. What
was actually wrong in `docs/tooling.md` was two byte-identical adjacent lines. The
wiki-link was incidental; the identical defect with any other content would be
equally wrong and equally uncaught.

## Decision

`criv check` does not flag a wiki-link repeated within a note. Repetition across
sections is legitimate and frequently correct.

The narrower rule — flag identical adjacent lines — is also rejected, on the
grounds established by
[[0046-no-native-linting-in-criv-enforce|ADR-0046]]. criv is a
documentation-to-code graph validator; general-purpose prose linting is not its
job, and a markdown linter already runs in the same pipeline. Adding
duplicate-line detection to `criv check` would re-import the scope creep ADR-0046
had just removed.

The concrete duplicated line was fixed directly.

## Consequences

Authors may link the same note from as many sections as the writing calls for,
which is what the ADR template's Context/Decision/Consequences structure tends to
produce.

A future audit that re-surfaces "repeated wiki-links are not caught" can read this
record instead of re-deriving it. Reopening needs evidence that repetition
correlates with real defects, not another single instance.

Duplicated prose remains the markdown linter's concern, or a reviewer's.

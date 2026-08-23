---
id: ADR-0136
kind: decision
title: Give util One Concern
status: accepted
date: 2026-08-23
governs:
  - src/util.rs
  - src/identity.rs
  - src/markdown.rs
---

# Give util One Concern

## Context

[[0105-owner-scoped-rust-module-layout|ADR-0105]] says a root filename should
name one complete concern. `src/util.rs` named none. It held seven unrelated
things: a fixture-copying test helper, path normalization, kebab-case
conversion, ADR id validation, wiki-link extraction, Markdown heading
extraction, and `GlobMatcher`.

A caller who learned that `util` existed learned nothing about what was in it,
so the module gave no leverage. It also changed 11 times in 200 commits, which
for a leaf helper file is the signature of a place where code lands when it has
no home.

`GlobMatcher` is the opposite: real behaviour behind a small interface,
including a tolerant compilation path for globset's automaton limit. It shared a
file with a fixture copier.

Deleting `src/util.rs` outright was the first plan. `criv check` refused it:
three accepted decisions list the path in `governs:`, and
[[0012-adr-immutability-enforcement|ADR-0012]] makes them immutable. Git detects
only a 43 per cent rename, and it maps to the wrong successor, so
`criv adr reconcile-sources` cannot absorb the move either. Superseding
[[0003-adopt-proven-foundation-crates|ADR-0003]],
[[0095-operating-system-watch-session-lock|ADR-0095]], and
[[0127-own-repository-files-behind-one-interface|ADR-0127]] to rename one file
would retire three decisions that are still correct about everything else.

## Decision

Keep `src/util.rs` and reduce it to one concern: glob matching. `GlobMatcher`
and nothing else stays there.

Move the rest to modules that name what they hold:

- `src/markdown.rs` owns `find_wiki_links_with_lines` and `markdown_headings`,
  the two note-body parsers. `src/vault.rs` is their only consumer.
- `src/identity.rs` owns `kebab`, `is_adr_id`, `strip_prefix`, and the
  test-only `copy_fixture_tree`. These answer what a note, an ADR, or a
  repository path is called.

No behavior changes. Every function keeps its signature and its tests.

The filename stays wrong on purpose. `util` now holds one concern, and the name
is the last thing left to correct. Correcting it costs three accepted decisions,
which is more than a name is worth today.

## Consequences

`src/util.rs` is 104 lines holding one type, where it was 274 lines holding
seven unrelated items. The three ADRs that govern it keep resolving.

A future helper with no obvious home has nowhere generic to land, because
`util` is no longer generic. That is the point: it forces a decision about
which concern owns it.

Renaming `src/util.rs` to `src/glob.rs` becomes cheap the next time one of the
three governing decisions is superseded for its own reasons. Until then, the
cost is a name.

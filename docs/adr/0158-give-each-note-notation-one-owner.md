---
id: ADR-0158
kind: decision
title: Give Each Note Notation One Owner
status: accepted
date: 2026-11-05
governs:
  - src/identity.rs
  - src/markdown.rs
---

# Give Each Note Notation One Owner

## Context

[[0139-name-the-glob-module-for-what-it-holds|ADR-0139]] gave
`src/markdown.rs` the two note-body parsers and gave `src/identity.rs` the
name conversions. Other modules kept their own copies of the same notations.

- The vault, `check`, `query`, and the State projection each split a wiki-link
  body on `|`, `#`, `source:`, and `match:` by hand.
- `src/adr.rs` had three wiki-link scanners, two frontmatter readers, and three
  rules for "this path is a decision". The rules disagreed on the case of the
  `.md` extension and on whether the decision directory came from `criv.toml`.
- The `ADR-NNNN` id and the `NNNN-slug.md` filename were formatted and parsed
  in eight places.
- A heading slug had two rules: `kebab` kept only ASCII letters and digits, and
  the title check kept all Unicode letters and digits.
- The pattern id `ADR-NNNN/local-id` was built in four places. The policy scan
  used the trimmed local id for violations and the untrimmed local id for State,
  so a padded id named two different patterns.

## Decision

`src/identity.rs` owns the names of things:

- the `ADR-NNNN` id, its number, and the largest number;
- the ADR filename `NNNN-slug.md` and its parser;
- the heading slug `slug`, which joins the lowercase Unicode alphanumeric
  words with `-`; it replaces `kebab`;
- the policy pattern id `ADR-NNNN/local-id`. The local id is trimmed, and a
  local id that is empty after the trim names no pattern.

`src/markdown.rs` owns the notations of a note:

- the `[[...]]` span scanner, with and without the code-block filter;
- `LinkTarget`, the one parser of a wiki-link body:
  `target[#fragment][|alias]`, `source:path[#symbol]`, `path#Lx-Ly`,
  `match:ADR-NNNN/id`, and `note#match:ADR-NNNN/id`;
- the frontmatter splitter and its top-level scalar reader;
- byte-offset to line conversion.

`Config::is_adr_path` is the one rule for a decision path: Markdown under the
configured ADR directory, other than its `README.md`. The immutability gate,
ADR reconciliation, the scope-move check, and `check` use it.

This refines the module ownership of ADR-0139. It does not change the glob,
watch-lock, or repository-file decisions that ADR-0139 carries forward.

## Consequences

A notation changes in one file. The callers ask the owner instead of slicing
strings.

A padded policy id, such as `id: " no-print "`, now names `ADR-NNNN/no-print`
in State, in links, and in violations.

Heading node ids in State change for headings with non-ASCII letters, because
the slug keeps them. The first refresh after the upgrade rewrites those ids.

The scope-move check recognizes a decision by the old and the new configured
ADR directory, not by a path component named `adr`.

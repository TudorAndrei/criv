---
id: ADR-0045
kind: decision
title: Note Line Identity In Generated State
status: accepted
date: 2026-07-25
governs:
  - src/vault.rs
  - src/c4.rs
  - src/state.rs
  - src/check.rs
---

# Note Line Identity In Generated State

## Context

Vault notes carry YAML frontmatter, and `src/vault.rs#fn:split_frontmatter`
strips that block before the body is parsed. Wiki-link, heading, and Mermaid C4
diagram positions are then computed over the stripped body, and the frontmatter
offset is never added back.

Every consumer treats those numbers as file lines. `criv check` anchors
`broken-link`, `source-wikilink`, `ambiguous-source-link`,
`non-portable-note-link`, and the C4 element and relationship diagnostics to
them. The GitHub annotation format, the VS Code diagnostic collection, and
Obsidian's jump-to-line all resolve them against the file on disk. Heading graph
nodes in `src/state.rs` embed them directly in the node identifier as
`path#L<line>:H<level>`.

Because frontmatter is required on vault notes, every one of those positions is
wrong by the length of the frontmatter block. In this repository the H1 of
`docs/adr/0001-local-cli-vault-architecture.md` sits on file line 14 and is
recorded as `#L2:H1`, an offset of exactly the twelve-line frontmatter block.
Diagnostics point into the frontmatter rather than at the reported problem.

Standalone `.c4` artifacts are unaffected. They have no frontmatter and parse
from line zero over the whole file, which is why the defect stayed invisible in
the C4 artifact path.

[[0007-content-addressed-state-and-diffing|ADR-0007]] made state node and edge
hashes the basis for snapshot comparison, and asked that the state schema stay
stable enough for the Obsidian companion and future diff consumers, preferring
additive change over incompatible rewrite. Correcting the offset does not change
the schema's shape, but it does change node identity.

## Decision

Line numbers criv reports for note content are file-relative. A position
reported for a wiki-link, heading, or C4 diagram element identifies the line in
the note file as it exists on disk, not a position within the parsed body.

The frontmatter offset is applied once, where parsed positions are attached to
the note in `src/vault.rs#fn:parse_note`. Parsers that operate on body
text keep returning body-relative positions; translation is the note layer's
responsibility, so a parser cannot double-count and a new parser inherits the
behavior.

A note whose frontmatter fails to parse is a deliberate exception. That path
retains the entire file as the body, so its offset is zero and its positions are
already file-relative.

Heading graph node identifiers keep the existing `path#L<line>:H<level>` shape
and adopt the corrected line. Identity is allowed to change here because the
prior identifier encoded a position that did not exist in the file; preserving
it would mean preserving a wrong answer to keep hashes stable.

## Consequences

Diagnostics from the CLI, GitHub annotations, VS Code, and Obsidian point at the
line a reader would count in the file.

Every heading node identifier for a note with frontmatter changes once, so the
first rebuild after this decision produces a large state diff and
`criv query diff` across that boundary reports each affected heading as removed
and added. Snapshots taken before the change cannot be meaningfully compared to
later ones at heading granularity. This is a one-time correction, not a schema
migration: the state schema version is unchanged because the shape, field names,
and hashing scheme are unchanged.

Future position-bearing note content must be attached through the same note
layer, so that it inherits the offset rather than reintroducing a body-relative
value.

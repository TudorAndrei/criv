---
id: ADR-0148
kind: decision
title: Separate Markdown Formatting From Vault Validation
status: accepted
date: 2026-10-30
governs:
  - src/check.rs
  - src/check/document.rs
  - src/diagnostic.rs
  - src/discovery/mod.rs
  - src/init/templates.rs
  - Cargo.toml
  - hk.pkl
  - package.json
---

# Separate Markdown Formatting From Vault Validation

## Context

Embedding rumdl makes criv own general Markdown style linting and formatting.
The current rumdl release still requires unmaintained unsafe-libyaml. Replacing
its formatting with an external formatter inside criv would instead make
ordinary vault validation depend on another installed executable.

A predictable document standard remains important. Formatting tools cannot
establish required frontmatter, a title heading, ADR sections, or valid links.

## Decision

Remove rumdl, its general Markdown discovery profile, and `check --fix`.
Both full and staged checks are read-only. Keep criv's vault, source, policy,
frontmatter, and link validation native; neither command invokes a formatter.
Refine the formatting clauses of ADR-0044, ADR-0067, ADR-0100, ADR-0111, and
ADR-0122 without changing their other contracts.

Require non-empty `id`, `kind`, and `title` frontmatter. Supported kinds remain
`doc` and `decision`. Decisions also require a supported status. A note has
exactly one H1 matching its title after case and punctuation normalization.
An active ADR has exactly one H2 `Context`, `Decision`, and `Consequences`, in
that order. Other H2 sections may appear between them. Fenced examples do not
count as headings. Broken heading links remain validation errors.

An accepted ADR made historical by an accepted successor keeps its original
bytes. Its successor must meet the current heading standard. Drafts and all
active ADRs are subject to the standard, regardless of creation date. Retain
frontmatter and link checks for historical records. Create compliant successors
for existing active exceptions rather than rewriting accepted decisions.

Report structural failures as `document-format`, with the note path and a
heading line when available. Repairs describe the document contract; they do
not invoke a general formatter.

Use `oxfmt` independently in repository Markdown formatting hooks. Configure
formatting scope through the formatter and hook configuration, not `.rumdl.toml`.
Exclude accepted ADR paths from automatic formatting to preserve immutability.
Repositories using criv may choose their own formatter or none at all.

Retire the rumdl Markdown performance probe. Historical adapter patches and
measurements remain historical evidence, not current product behavior.

## Consequences

A valid vault needs no formatter, Node.js, or npm unless it has LikeC4 source.
General style issues do not fail criv, but missing document structure does.
Root Markdown outside the vault is no longer interpreted as a criv note.

Existing consumers must remove `check --fix` from hooks and run their own
formatter separately. Style-rule suppression comments and rumdl file selection
no longer affect criv. This is an intentional CLI and validation change.

Removing rumdl also removes its unsafe-libyaml dependency path. No advisory
exception is needed. ast-grep's replacement YAML parser requires `encoding_rs`;
allow its permissive BSD-3-Clause license in the dependency gate.

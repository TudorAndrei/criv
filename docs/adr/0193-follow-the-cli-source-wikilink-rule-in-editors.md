---
id: ADR-0193
kind: decision
title: Follow the CLI source Wikilink rule in editors
status: accepted
date: 2026-11-06
governs:
  - packages/criv-editor-state/src/sourceTarget.ts
  - extensions/vscode-criv/src/navigation/references.ts
  - extensions/vscode-criv/src/navigation/languageFeatures.ts
  - .obsidian/plugins/criv/src/source/references.ts
---

# Follow the CLI Source Wikilink Rule in Editors

## Context

[[0034-ast-aware-source-selectors|ADR-0034]] made AST-aware selectors the
canonical form of a code reference and reserved Wikilinks for documents.
`criv check` applies this in two rules in `src/check.rs`. `source-wikilink`
warns for each Wikilink that resolves to source. `legacy-source-target` warns
for a frontmatter `targets.symbols` entry that uses `source:` or a
non-canonical selector.

The editors had a third rule. VS Code reported every source Wikilink as
`legacy-source-target`, with one of two messages that depended on whether the
link text was already canonical. The VS Code hover added a third sentence.
VS Code also read `%% criv:source` directives, a convention that
[[0155-remove-likec4-from-criv|ADR-0155]] removed. The Obsidian completion
inserted a selector inside `[[`, so each completion wrote the form that
`criv check` warns about.

## Decision

`@criv/editor-state` owns the editor form of the CLI `source-wikilink` rule:
the code `SOURCE_WIKILINK` and the message `sourceWikilinkWarning`. A change to
the CLI rule in `validate_note_links` changes these two together.

VS Code reports a Wikilink that resolves to source as a `source-wikilink`
warning with that message, and its hover shows the same sentence. A bare
selector that resolves gets no warning. A Wikilink that names a file or a
`source:` target and does not resolve, or resolves ambiguously, keeps its
unresolved or ambiguous diagnostic. VS Code does not read `criv:source`
directives.

The Obsidian completion still opens after `[[`. Selecting a suggestion replaces
the whole Wikilink, with its closing `]]`, by the bare selector.

The frontmatter `legacy-source-target` rule stays in the CLI only. The editors
do not read frontmatter targets.

## Consequences

An editor warning and the `criv check` warning for the same source Wikilink
have the same code and the same fix. A completion writes the form that
`criv check` accepts.

A source Wikilink that VS Code used to describe as legacy now has the CLI
message. VS Code does not resolve notes before source, so a note link written
with a file extension, such as `[[name.ext]]`, can still get an unresolved
source warning that the CLI does not give.

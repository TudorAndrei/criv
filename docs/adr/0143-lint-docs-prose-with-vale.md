---
id: ADR-0143
kind: decision
title: Lint docs prose with Vale
status: accepted
date: 2026-09-25
governs:
  - .vale.ini
  - .vale/styles/config/vocabularies/criv/accept.txt
  - hk.pkl
  - mise.toml
  - mise.lock
  - .github/workflows/ci.yml
---

# Lint docs prose with Vale

## Context

`criv check` and rumdl validate the structure of the `docs/` vault: frontmatter,
links, and Markdown syntax. No check reads the prose. Spelling errors and
inconsistent term case can go into the vault without a failure.

The maintainer requested [Vale](https://github.com/vale-cli/vale) as the prose
linter for `docs/`, and the official
[vale-action](https://github.com/vale-cli/vale-action) in hosted CI.

## Decision

Pin Vale 3.22.0 through `mise.toml` with the `aqua:vale-cli/vale` backend.

`.vale.ini` applies only the built-in `Vale` style to Markdown. The built-in
style needs no downloaded packages, so no step runs `vale sync`. Local and
hosted checks are deterministic and work offline.

The project vocabulary is
`.vale/styles/config/vocabularies/criv/accept.txt`. Proper nouns and acronyms,
such as `Git`, `JSON`, `Wasm`, `ADR`, and `CI`, have no prefix, so `Vale.Terms`
enforces their case. Common words have the `(?i)` prefix, so a capital letter at
the start of a sentence or title is correct. Add a term to the vocabulary when
Vale flags a correct project word.

`.vale.ini` ignores wiki-link targets through `TokenIgnores`, because a target
is a file name or a match ID, not prose. Put tool and package names in code
spans. To keep an upstream quotation unchanged, turn off the rule around it
with a `<!-- vale Vale.Terms = NO -->` comment.

The first run rewrote existing notes and ADRs to agree with Vale. This change
only corrected term case and code spans; it did not change a decision.

hk runs Vale in `pre-commit` on changed files under `docs/`.

Hosted CI runs Vale in a separate `vale` job through `vale-cli/vale-action`,
pinned to a full commit SHA. The job lints all of `docs/` with
`filter_mode: nofilter`, fails on errors, and posts annotations on the pull
request. The `Repository checks` gate requires the job. This job is an
exception to [[0049-checks-defined-in-hk-not-mise|ADR-0049]]: the action gives
pull request annotations that `hk check` cannot give. The hk `check` profile
does not run Vale, so hosted CI runs it one time.

## Consequences

A docs change that adds an unknown word fails the local commit and CI until the
word is corrected or added to the vocabulary.

The action pins the same Vale version as `mise.toml`. Update both pins in the
same change.

Stricter styles, such as write-good or a controlled-language style, need
`vale sync` or checked-in style files. Adding one requires a new decision.

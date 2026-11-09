---
id: ADR-0180
kind: decision
title: Keep Installation Safe without Caller Checks
status: accepted
date: 2026-11-06
governs:
  - src/init.rs
  - src/init/templates.rs
  - src/install.rs
  - src/install/skills.rs
  - src/repository/filesystem.rs
  - src/config.rs
  - src/lib.rs
---

# Keep Installation Safe without Caller Checks

## Context

`RepositoryWriteScope::link_dir` replaced a directory at the link path when
asked to refresh. A Windows junction to `.agents/skills` reads as a directory,
so a refresh could remove the skills through it. Only the caller's check that
the link was already in place kept this from happening.

`criv init` named `docs/adr` and `README.md` as literals beside
`Config::default`. `--no-skills --force-skills` did nothing and succeeded. The
installed-skill inventory had five public states, but its only reader counted
outdated paths. Two command-line failures were printed by the parser and the
rest by `main`.

## Decision

- `link_dir` returns `Unchanged` when the link path already resolves to its
  target, before it looks at what kind of entry the path is. Callers do not
  check first.
- `criv init` takes the decision directory and its index path from
  `Config::default`. `Config` owns the index file name. A test requires the
  template to state every field of `Config::default`.
- `--no-skills` and `--force-skills` conflict at parse time.
- The skills inventory returns the outdated paths only. Skill frontmatter is
  split with `markdown::split_frontmatter`.
- A usage failure is one `CrivError::Usage` value holding the rendered text,
  which `main` prints as it is.

## Consequences

A refresh cannot remove the skills through a junction or another link that
resolves to them.

`criv init --no-skills --force-skills` exits with code 2.

A usage failure no longer starts with `criv:`.

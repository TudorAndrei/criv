---
id: ADR-0082
kind: decision
title: No Standalone Search Command
status: accepted
date: 2026-08-06
supersedes:
  - ADR-0005
  - ADR-0047
governs:
  - Cargo.toml
  - src/lib.rs
  - src/config.rs
  - src/init/templates.rs
  - src/source/index.rs
  - scripts/performance/src/main.rs
  - assets/skills/**
  - .agents/skills/**
---

# No Standalone Search Command

## Context

The standalone search command combined file-name matching, source grep, note
ranking, ad hoc structural matching, and policy-pattern inspection. These modes
did not form one stable product boundary. File and text search duplicate common
repository tools. Policy evaluation already belongs to `criv check` and `criv
enforce`. Typed graph inspection belongs to `criv query`.

[[0047-semantic-note-search-stays-source-only|ADR-0047]] also kept an optional
note-search path that was not available in release binaries. That path added a
Cargo feature, a configuration key, an inference dependency tree, a model
download, and a runtime download. It increased the measured binary size from
12,332,560 bytes to 30,482,144 bytes. Its first use needed a network connection
and created a 97 MB cache.

Keeping the standalone command would retain a broad CLI surface and maintenance
cost without one clear responsibility. Keeping the optional note-search path
would also conflict with the small, offline release model.

## Decision

Remove the standalone `criv search` command and all its file, grep, note,
structural, rule, and pattern-ID modes. Remove the command module, CLI-only
source-index grep API, regex dependency, command tests, performance case, user
documentation, runtime-skill guidance, and architecture nodes.

Remove the optional note-search Cargo feature, inference dependency, model and
runtime initialization, configuration field, generated configuration entry,
tests, and current user documentation. An obsolete configuration entry has no
effect, in the same way as other removed configuration entries.

Keep the internal source catalog and partial-path matcher because source-link
resolution, state publication, and editor consumers use them. Keep structural
policy matching with `ast-grep-core` inside checks, enforcement, and state
generation. Accepted ADR policies remain active enforcement rules over their
effective `governs:` scopes. Keep inline ADR policies as the only persistent
named patterns, and keep accepted-only policy registration in generated state.

Do not remove an existing local model cache. criv does not own user cleanup and
will no longer create or read that cache.

## Consequences

The CLI command surface has no general search command. Users can use repository
tools for file and text lookup, `criv query` for typed graph questions, and a
filtered `criv check` for one decision's policy diagnostics.

Release and source builds have the same capabilities. The CLI has no model
download, inference runtime download, optional inference dependency tree, or
CLI-only regex dependency.

Internal fuzzy path resolution and structural policy evaluation remain
implementation services. They do not expose a general search interface.

A future search command or non-lexical note-retrieval feature requires a new
decision with a narrow responsibility and measured installation size, memory
use, runtime cost, and retrieval quality.

---
id: ADR-0111
kind: decision
title: File Discovery Compatibility Contract
status: accepted
date: 2026-08-14
governs:
  - src/check.rs
  - src/config.rs
  - src/source/index.rs
  - src/source/paths.rs
  - src/util.rs
  - src/vault.rs
  - src/watch.rs
---

# File Discovery Compatibility Contract

## Context

criv discovers files for three different purposes. Source discovery builds the
source catalog. Vault discovery loads Markdown notes and LikeC4 files from the
configured docs directory. Markdown discovery selects repository files for
rumdl linting and fixes.

These paths use different traversal code and have inconsistent implicit
behavior. Source selection changes when `fff-search` finds a Git repository.
Repository Markdown discovery silently drops walk errors and invalid selection
patterns. Several paths convert names to lossy UTF-8. Vault discovery has a
stricter link and error policy than the other paths.

[[0006-fff-source-index-and-incremental-watch|ADR-0006]] and
[[0042-shared-source-index-lifecycle|ADR-0042]] make one source catalog
authoritative. [[0044-vault-write-confinement|ADR-0044]] requires a strict vault
boundary and makes rumdl configuration the Markdown fix scope. This decision
defines the file-selection contract that a later traversal architecture must
implement. It does not select a traversal library.

## Decision

Define three named discovery profiles: **Source**, **Vault**, and **Markdown**.
They can use one shared discovery interface and traversal engine, but they must
not use one universal selection policy. A platform-specific engine is valid
only when it implements the same profile behavior.

All profiles inspect the live worktree. They include untracked files when those
files meet the profile rules. The term "committed vault" identifies repository
content in the configured docs directory. It does not limit discovery to the
Git index or a Git tree.

### Source profile

Use `source.roots` and `source.exclude` as the source selection authority. A
root can name a directory or one explicit file. A missing root produces no
files and no error. Exact and nested roots produce one duplicate-free result
set. An implementation can combine nested scans and can use excludes to prune
traversal when this does not change that set.

Include hidden and Git-ignored text files that are under a configured source
root and are not excluded by `source.exclude`. Always exclude `.git` and
`.criv` directories. Do not let the presence of a Git repository change source
selection.

Exclude binary files. Define text and binary compatibility with shared golden
fixtures rather than with one dependency's classifier. An invalid source
exclude is an error.

### Vault profile

Walk the configured docs directory without Git ignore rules, rumdl rules, or
source excludes. Include hidden files. Always skip real directories named
`.git`, `.criv`, `target`, and `node_modules` at any depth.

Select exact lowercase `.md` and `.c4` extensions for their respective vault
consumers. A missing docs directory produces an empty set. One traversal can
route Markdown and C4 results to their respective parsers.

### Markdown profile

Select `.md` and `.markdown` extensions without letter-case sensitivity. Keep
rumdl `include`, `exclude`, and `respect_gitignore` settings as the selection
authority. An explicit include can select a hidden path. An invalid include or
exclude pattern is an error.

Markdown lint and `check --fix` use the same selected set. A traversal
optimization for `check --changed` is valid when it preserves the promotion
and validation rules in
[[0067-staged-changes-are-a-partial-check-scope|ADR-0067]].

### Shared behavior

Do not follow symbolic links or Windows junctions. A link within a profile's
selected scope is an error. A link outside that profile's scope has no effect.
An unreadable path or traversal error within selected scope is an error. An
error in a subtree that every active profile has pruned has no effect.

Represent each selected path as a repository-relative path with `/`
separators. Preserve letter case and use case-sensitive identity. Reject a path
that cannot be represented as UTF-8. Do not use lossy conversion. Remove
duplicates and return paths in stable lexical order on every supported
platform.

Keep current separator-sensitive glob behavior. A replacement matcher must
pass one shared compatibility corpus.

Source target lookup guarantees exact-path lookup, suffix or basename lookup,
explicit ambiguity, and deterministic lexical selection. Fuzzy ranking and
frecency are implementation details. They are not compatibility requirements.

One-shot commands and live watch use the same Source selection rules. After
filesystem activity stops, they must produce the same sorted file set.

Use one shared profile fixture corpus on Linux, macOS, and Windows. It must
cover hidden and ignored paths, nested and explicit roots, binary files,
excludes, links and junctions, unreadable paths, invalid patterns, path
ordering, and supported non-UTF-8 cases.

## Consequences

The contract removes environment-dependent Source selection. A Git checkout
and an equivalent non-Git tree select the same Source files. Hidden and
Git-ignored files under an explicit Source root become valid unless an explicit
source exclude removes them.

Markdown discovery becomes strict for invalid patterns and selected-scope walk
errors. Explicit rumdl includes can select hidden files. Repository Markdown
extensions no longer depend on a MIME table that can change in a dependency
update.

Nested Source links and selected Markdown links that were silently omitted now
produce errors. Non-UTF-8 paths that were converted with possible identity loss
now produce errors. These are intentional correctness changes.

The implementation can share traversal work and prune excluded trees. It still
needs separate policy adapters for the three profiles. Backend choice,
performance gates, release costs, and any `fff-search` or zlob replacement
remain separate decisions.

## Alternatives Considered

### Use one selection policy for every consumer

Rejected. The Source, Vault, and Markdown scopes have different configuration
authorities and safety requirements.

### Preserve every observed backend detail

Rejected. Current Source selection changes between Git and non-Git roots.
Silent Markdown errors and lossy path conversion can give an incomplete or
ambiguous result. These details are defects, not required compatibility.

### Select only files tracked by Git

Rejected. criv validates the live worktree and must see eligible untracked
files before they are committed.

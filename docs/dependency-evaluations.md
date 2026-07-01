---
id: dependency-evaluations
kind: doc
title: Dependency evaluations
---

# Dependency evaluations

These evaluations cover foundation crates that are useful but not required for
the current parser-backed implementation.

## miette

Decision: defer.

`miette` is a good fit for future source-span diagnostics because it provides a
diagnostic protocol, source snippets, labels, related diagnostics, and optional
fancy reports. `criv` diagnostics currently carry line numbers but not byte
spans or source snippets, so adopting it now would mostly add dependency weight
without improving output. Revisit once check diagnostics store source offsets.

Reference: <https://lib.rs/crates/miette>

## infer

Decision: defer until plugin asset previews are implemented.

`infer` detects file types from magic-number signatures and returns MIME and
extension metadata. The CLI already skips binary source files with
`content_inspector` and records cheap extension MIME hints with `mime_guess`.
Magic-number detection becomes useful when the Obsidian plugin previews
non-source assets from state.

Reference: <https://lib.rs/crates/infer>

## serde_yaml_ng

Decision: keep `serde_norway`; do not add `serde_yaml_ng` now.

`serde_yaml_ng` is a serde-yaml fork and a viable alternate YAML backend, but
the current implementation already replaced deprecated `serde_yaml` with
`serde_norway`. Keep a single YAML parser until frontmatter compatibility tests
show a concrete gap.

Reference: <https://lib.rs/crates/serde_yaml_ng>

## camino

Decision: defer until repo-relative path APIs are refactored.

`camino` provides UTF-8 path types that avoid repeated lossy conversions. That
matches `criv`'s repo-relative path invariant, but adopting it cleanly should be
done as a focused path-type refactor across config, vault, state, search, and
query modules.

Reference: <https://docs.rs/camino>

## Cargo Audit Snapshot, 2026-07-01

Decision: document and monitor; do not add a failing `cargo audit` gate yet.

`cargo audit --no-fetch` loaded the local advisory database and reported four
allowed warnings in `Cargo.lock`. The command also warned that it could not open
the crates.io index cache lock, so this is useful dependency posture signal, not
a clean hosted-audit baseline.

The actionable paths are currently transitive:

- `git2 v0.20.4` reaches criv through `fff-search v0.9.6`; criv's direct
  `git2` dependency is already `0.21`.
- `bincode v1.3.3` reaches criv through `heed-types -> heed -> fff-search`.
- `paste v1.0.15` is present in `Cargo.lock` through transitive/optional
  dependency paths, including `macro_rules_attribute` and `tokenizers`; it does
  not appear in the default `cargo tree` output.

Evidence commands:

```sh
cargo audit --no-fetch
cargo tree -i git2@0.20.4
cargo tree -i git2@0.21.0
cargo tree -i bincode
```

Do not add an ignore list or CI gate until the team decides whether the
`fff-search` and optional embeddings dependency trees are acceptable as-is,
upgradable in place, or need a replacement/spike. A future audit gate should use
a reproducible advisory database update path rather than relying on the local
developer cache.

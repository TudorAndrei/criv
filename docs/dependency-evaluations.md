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

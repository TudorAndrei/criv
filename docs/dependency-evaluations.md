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

## Cargo Audit Snapshot, 2026-07-23

Decision: document and monitor; do not add a failing `cargo audit` gate yet.

The pinned `cargo-audit v0.22.2` command was run as `cargo audit --no-fetch`.
It loaded 1,169 advisories from the local advisory database at commit
`1abf7a8c1822223a38e99f652bc232071c44a86d` (2026-07-23 09:15:03 +02:00)
and scanned 461 locked packages. It reported four allowed warnings, all listed
below. None was classified as a vulnerability by this run.

This is a dated, local posture snapshot rather than a hosted-audit baseline:
`--no-fetch` intentionally does not update the advisory database, and the
command warned that it could not open the crates.io index cache lock. A future
policy gate needs a reproducible advisory-database update path before it can be
relied on in CI.

### Unsound APIs: `git2 v0.20.4`

`RUSTSEC-2026-0183` reports potential undefined behavior when
`Remote::list()` is called, and `RUSTSEC-2026-0184` reports potential undefined
behavior for a `Signature` obtained from a buffer-created `BlameHunk`. Both
affect `git2 v0.20.4`, which reaches criv only through
`fff-search v0.10.1`:

```text
git2 v0.20.4 <- fff-search v0.10.1 <- criv
```

`criv` has no direct `git2` dependency; `cargo tree -i git2@0.21.0` finds no
resolved package. The 0.9.6 version and direct-`git2` statement in the prior
snapshot were stale.

The locally installed `fff-search v0.10.1` source was inspected. Its git path
uses `Repository::open`, status enumeration, `workdir`, and `status_file`; a
source search found no invocation of `Remote::list`, `BlameHunk`, or blame APIs.
That is evidence that the two advisory call paths are not reached by the
currently inspected source, not proof that text search alone can rule out every
runtime path or upstream behavior.

### Unmaintained crates: `bincode` and `paste`

`RUSTSEC-2025-0141` marks `bincode v1.3.3` unmaintained. Its active default
dependency path is:

```text
bincode v1.3.3 <- heed-types v0.21.0 <- heed v0.22.1 <- fff-search v0.10.1 <- criv
```

`RUSTSEC-2024-0436` marks `paste v1.0.15` unmaintained. It is present in the
lockfile but absent from the default and default-target dependency trees. The
feature/target tree shows it becomes active only with criv's optional
`embeddings` feature:

```text
paste <- macro_rules_attribute <- tokenizers <- fastembed <- criv[embeddings]
```

This distinguishes an inactive default-build lockfile entry from an absent
dependency: embedding builds still use it and remain in the monitoring scope.

### Policy conclusion

The monitor-only decision is unchanged. The current findings are two
unmaintained crates and two potentially unsound-but-unreached APIs, not a new
vulnerability classification or demonstrated runtime exploit path. Do not add
an audit ignore list or failing gate, and do not replace `fff-search`, without a
separate approved decision. Because this policy did not change, no new ADR is
needed and accepted ADRs remain unmodified.

Evidence commands:

```sh
cargo audit --no-fetch
cargo tree -i git2@0.20.4
cargo tree -i bincode@1.3.3
cargo tree --all-features --target all -e features -i paste@1.0.15
```

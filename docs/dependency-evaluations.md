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

`miette` is a good fit for a future human diagnostic renderer because it
provides source snippets, labels, related diagnostics, and optional fancy
reports. [[0122-byte-spans-and-lsp-diagnostic-ranges|ADR-0122]] defines the
source-span contract, but it keeps renderer selection separate. Revisit
`miette` after the main diagnostic producers preserve exact spans and a
renderer change shows enough user value to justify the dependency.

Reference: <https://docs.rs/miette/latest/miette/struct.SourceSpan.html>

## infer

Decision: use for the bounded documentation asset inventory.

`infer` detects file types from magic-number signatures and returns MIME and
extension metadata. [[0131-publish-verified-documentation-assets-for-native-previews|ADR-0131]]
requires the CLI to verify the file signature and the extension before it adds
an asset to State. `infer` supplies this narrow check. `content_inspector`
continues to classify Source text, and `mime_guess` continues to supply Source
MIME hints. The asset inventory does not use either dependency as a security
check.

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

`criv` directly depends on `git2 v0.21.0` with default features disabled for
local repository discovery, tree/index/worktree diffs, commit traversal, and
blob reads. It does not create transports or invoke the advisory APIs. The
older `git2 v0.20.4` remains an independent transitive dependency of
`fff-search`; `cargo tree -i git2@0.21.0` and `cargo tree -i git2@0.20.4`
distinguish the two paths.

The locally installed `fff-search v0.10.1` source was inspected. Its git path
uses `Repository::open`, status enumeration, `workdir`, and `status_file`; a
source search found no invocation of `Remote::list`, `BlameHunk`, or blame APIs.
That is evidence that the two advisory call paths are not reached by the
currently inspected source, not proof that text search alone can rule out every
runtime path or upstream behavior.

### Unmaintained crate: `bincode`

`RUSTSEC-2025-0141` marks `bincode v1.3.3` unmaintained. Its active default
dependency path is:

```text
bincode v1.3.3 <- heed-types v0.21.0 <- heed v0.22.1 <- fff-search v0.10.1 <- criv
```

### Policy conclusion

The monitor-only decision is unchanged. The current findings are two
unmaintained crates and two potentially unsound-but-unreached APIs, not a new
vulnerability classification or demonstrated runtime exploit path. Do not add
an audit ignore list or failing gate, and do not replace `fff-search`, without a
separate approved decision. The embedded repository backend is governed by its
own ADR; accepted audit-policy ADRs remain unmodified.

[[0055-dependency-auditing-in-hk-checks|ADR-0055]] subsequently runs this same
command as a visible, non-blocking hk monitor. It does not change this Rust
decision or make `cargo audit` a failing gate.

## Embedded Git backend measurement, 2026-08-02

`git2 v0.21.0` is a direct `MIT OR Apache-2.0` dependency with default features
disabled. Its native `libgit2-sys v0.18.7+1.9.6` dependency has the same license
expression. The resolved graph deliberately contains both `git2 v0.21.0` for
criv's local repository boundary and `git2 v0.20.4` through `fff-search`; Cargo
resolves both wrappers onto the same `libgit2-sys` version.

Same-toolchain, clean, size-optimized release builds on this macOS host measured
the `main` binary at 12,702,432 bytes in 1:14.62 and this branch at 12,737,152
bytes in 1:59.65. The embedded backend therefore adds 34,720 bytes (0.27%). The
branch build is slower by 45.03 seconds in this cold local comparison; this is
recorded for release review, not treated as a release blocker.

`cargo audit --no-fetch` on 2026-08-02 reported the same four allowed warnings:
the two `git2 v0.20.4` advisory paths and the existing `bincode` and `paste`
maintenance warnings. It reported no advisory for direct `git2 v0.21.0`.

Evidence commands:

```sh
cargo audit --no-fetch
cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "git2" or .name == "libgit2-sys") | "\(.name) \(.version): \(.license // "NOASSERTION")"'
cargo tree -i git2@0.21.0
cargo tree -i git2@0.20.4
cargo tree -i bincode@1.3.3
cargo tree --all-features --target all -e features -i paste@1.0.15
```

## File discovery resolution, 2026-08-16

[[0112-direct-ignore-file-discovery|ADR-0112]] removes `fff-search` and its
transitive `git2 v0.20.4`, `heed`, and `bincode v1.3.3` path. Source, Vault,
and Markdown selection now use the existing pure-Rust `ignore`, `globset`, and
`content_inspector` dependencies. Direct `git2 v0.21.0` remains the embedded
repository boundary.

The dated audit and build measurements above remain historical evidence. Their
instruction to keep `fff-search` was superseded only after the separate file-
discovery contract, implementation decision, compatibility corpus, and release
gates were accepted.

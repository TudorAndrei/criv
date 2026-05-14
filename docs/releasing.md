---
id: releasing
kind: doc
title: Releasing criv
---

# Releasing criv

Use Cocogitto to calculate the next SemVer version from conventional commits,
then use `cargo-release` to update workspace Cargo versions.

Before preparing a release, run:

```sh
cargo test --workspace
cargo fmt --check
target/debug/criv check
target/debug/criv enforce --stage ci
target/debug/criv watch --once
target/debug/criv query diff latest latest
```

`target/debug/criv check` embeds `rumdl` as a Rust crate, so Markdown formatting
does not require a separate `rumdl` executable.

Build plugin artifacts when the Obsidian plugin is part of the release:

```sh
cd .obsidian/plugins/criv
npm run build
```

This runs `wasm-pack` through the plugin build script. If only the Rust CLI is
being released and the generated plugin scaffold is unchanged, document that
choice in the release notes.

Preview the next automatically selected version:

```sh
mise run release-plan
```

For a tag-only release:

```sh
mise run release-auto
```

`release-auto` is implemented by [[scripts/release-auto.sh]]. It requires a
clean `main` branch, asks Cocogitto for the next version with
`cog bump --dry-run --auto`, updates workspace Cargo versions, runs the
pre-release checks above, commits the version bump, creates `vX.Y.Z` and
`criv-wasm-vX.Y.Z`, and pushes the commit and tags.

Conventional commits drive the automatic bump: `fix` produces a patch release,
`feat` produces a minor release, and `!` or `BREAKING CHANGE:` produces a major
release. While criv is still in `0.y.z`, Cocogitto will not automatically select
`1.0.0`; cut that intentionally with a manual versioned release if needed. This
decision is captured in [[ADR-0016]].

For crates.io publishing, confirm the package metadata first:

```sh
cargo package --workspace --allow-dirty
cargo publish --dry-run
```

Then run the matching `cargo release` command without `--no-publish`. Crates.io
publishing remains manual for now; `release-auto` only cuts the tag-triggered
binary release.

Current tag names use:

- `vX.Y.Z` for the root CLI crate.
- `criv-wasm-vX.Y.Z` for the WASM helper crate.

Release binary automation should run only when a `v*` root CLI release tag is
pushed. The workflow should build `criv` archives named by Rust target triple:

- `criv-x86_64-unknown-linux-gnu.tar.gz`
- `criv-aarch64-unknown-linux-gnu.tar.gz`
- `criv-x86_64-apple-darwin.tar.gz`
- `criv-aarch64-apple-darwin.tar.gz`
- `criv-x86_64-pc-windows-msvc.zip`

Release assets should include `SHA256SUMS.txt`, GitHub build provenance
attestations, and `criv --version` as the installer smoke test for future aqua
and mise registry entries. This decision is captured in [[ADR-0014]].

Release binaries use the workspace release profile in [[Cargo.toml]]: symbols
are stripped, size optimization is enabled, LTO runs at link time, codegen uses
one unit, and release panics abort. This keeps downloadable artifacts smaller
without requiring nightly Rust or post-build binary packing. The profile
decision is captured in [[ADR-0015]].

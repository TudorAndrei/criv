---
id: releasing
kind: doc
title: Releasing criv
---

# Releasing criv

Use `cargo-release` for workspace release tagging and version changes.

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

For a tag-only release:

```sh
cargo release patch --workspace --no-publish
```

For crates.io publishing, confirm the package metadata first:

```sh
cargo package --workspace --allow-dirty
cargo publish --dry-run
```

Then run the matching `cargo release` command without `--no-publish`.

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

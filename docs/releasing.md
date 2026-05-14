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

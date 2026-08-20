---
id: releasing
kind: doc
title: Releasing criv
---

# Releasing criv

The `Automatic release` GitHub Actions workflow owns release preparation,
acceptance, and publication. A successful CI run for a qualifying push to
`main` starts it. No local computer or self-hosted runner is part of the
release boundary.

The workflow uses Cocogitto to calculate the next SemVer version from
Conventional Commits. It uses `cargo-release` to update all workspace Cargo
versions, pushes one version commit, and runs the complete repository checks
again on that exact commit. A push with no release change is a successful
no-op. GitHub generates the release notes. The repository does not keep a
generated changelog.

Preview the next automatically selected version:

```sh
mise run release-plan
```

The release workflow measures matched 100,000-entry Source workloads on all
four native hosts. Hosted macOS also runs the 250,000-entry Vault and Markdown
workloads and the live-watch gate. Ouro is an optional `mise run perf` workload
and does not block publication.

Each clean-build lane supplies the exact binary that enters the release. The
workflow publishes a seven-day receipt to `refs/notes/criv-release-gates`,
packages those measured bytes, verifies each archive on its native host, pushes
both tags atomically, uploads a draft, adds provenance attestations, and then
publishes it.

The first release with default Elixir support uses the v0.9.0 pre-Elixir
baseline. It permits `tree-sitter-elixir` as the only new normal package name
and records the full binary-size change. It does not use an Elixir speed limit.
Each native release binary must read and parse the complete `.ex` and `.exs`
coverage set. Later releases compare with the last stable release that has the
same Elixir evidence contract. Those later comparisons permit no new normal
package name, no normal dependency-count increase, and no binary growth.

Release notes for this transition must state that the Elixir grammar is in the
default binary. They must also state that criv reads configured `.ex` and
`.exs` roots without Mix and does not support EEx, HEEx, or macro expansion.

If a run fails, use the exact prepared commit and tag shown in the run:

```sh
gh workflow run release.yml --ref main \
  -f commit=<full-release-commit> \
  -f tag=vX.Y.Z
```

The retry accepts an untagged prepared commit, matching tags, or a matching
draft. It rejects tags that point to another commit. A receipt older than seven
days causes the evidence to run again.

An automatic retry reads the evidence contract from the exact prepared commit.
If that commit is older than the evidence-contract file, the workflow uses the
pre-Elixir contract, the v0.9.0 adapter, and the matching artifact schema. It
does not run Elixir-only coverage checks for that older commit. New workflow
steps must not require files that were added after the prepared commit.

Generated release workloads disable automatic Git maintenance and complete one
synchronous `git gc` before measurement. This keeps `.git/objects` stable while
the macOS copy-on-write snapshot is created.

Download raw measured binaries and evidence from a run with:

```sh
gh run list --workflow release.yml
gh run download <run-id> \
  --pattern 'release-evidence-*' \
  --dir release-evidence
```

The per-platform evidence, combined evidence bundle, and packaged release
candidate remain as Actions artifacts for 90 days. Published archives,
`SHA256SUMS.txt`, `release-gate.json`, and attestations remain with the GitHub
release. Download them without an Actions run ID:

```sh
gh release download vX.Y.Z --dir dist
gh attestation verify dist/criv-aarch64-apple-darwin.tar.gz \
  --repo TudorAndrei/criv
```

Conventional commits drive the automatic bump: `fix` produces a patch release,
`feat` produces a minor release, and `!` or `BREAKING CHANGE:` produces a major
release. While criv is still in `0.y.z`, Cocogitto will not automatically select
`1.0.0`; cut that intentionally with a manual versioned release if needed. This
decision is captured in [[0016-conventional-commit-semver-release-automation|ADR-0016]].

Releases remain git-tag-only. Do not publish `criv` to crates.io until
the CLI API, state schema compatibility policy, and installer story are stable
enough to support registry consumers. The GitHub binary release is
the authoritative distribution path for now.

When crates.io publishing is intentionally enabled later, confirm the package
metadata first:

```sh
cargo package --workspace --allow-dirty
cargo publish --dry-run
```

Then run the matching `cargo release` command without `--no-publish`. Crates.io
publishing remains manual for now. The prepared release commit and hosted tag
publication do not publish to crates.io.

The file-discovery release removes Source frecency data from Rust and editor
types and from new State output. The schema name stays `criv.state.v1`. New
readers accept an older document that contains the extra `frecency` field and
ignore that field. Release notes must identify this Rust source API change and
the intentional file-selection corrections from ADR-0111.

Current tag names use:

- `vX.Y.Z` for the root CLI crate.
- `criv-wasm-vX.Y.Z` for the WASM helper crate.

The workflow builds `criv` archives named by Rust target triple:

- `criv-x86_64-unknown-linux-gnu.tar.gz`
- `criv-aarch64-unknown-linux-gnu.tar.gz`
- `criv-aarch64-apple-darwin.tar.gz`
- `criv-x86_64-pc-windows-msvc.zip`

Intel macOS release archives are deprecated by [[0017-deprecate-intel-macos-release-archives|ADR-0017]] because the hosted
Intel macOS runner is the slowest release job. Apple Silicon macOS remains the
supported macOS binary target. Reintroduce `criv-x86_64-apple-darwin.tar.gz`
only if there is measured user demand or a faster runner path.

Release assets should include `SHA256SUMS.txt`, GitHub build provenance
attestations, and `criv --version` as the installer smoke test for future aqua
and mise registry entries. This decision is captured in [[0014-tag-triggered-release-binary-workflow|ADR-0014]].

Each platform archive also includes `vscode-criv.vsix` next to the executable.
The release workflow builds that package once and adds the same local-only
viewer to every archive, as required by
[[0087-keep-editor-setup-out-of-init|ADR-0087]].

Release binaries use the workspace release profile in `Cargo.toml`: symbols
are stripped, size optimization is enabled, LTO runs at link time, codegen uses
one unit, and release panics abort. This keeps downloadable artifacts smaller
without requiring nightly Rust or post-build binary packing. The profile
decision is captured in [[0015-size-optimized-release-profile|ADR-0015]].

The CLI embeds local Git repository access through its Rust dependency graph;
the release artifact does not require a `git` executable for query-diff or
enforcement repository reads. Release verification should keep a PATH-without-
Git smoke test alongside the hosted core validation profile. The backend scope
and dependency evidence are recorded by the embedded-repository-access ADR.

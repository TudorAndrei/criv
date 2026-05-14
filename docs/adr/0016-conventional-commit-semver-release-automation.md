---
id: ADR-0016
kind: decision
title: Conventional Commit SemVer Release Automation
status: accepted
date: 2026-05-15
governs:
  - cog.toml
  - mise.toml
  - scripts/release-auto.sh
---

# Conventional Commit SemVer Release Automation

## Context

criv already enforces conventional commit messages through [[hk.pkl]] and uses a
tag-triggered binary workflow in [[ADR-0014]]. The release process in
[[releasing]] still required choosing `major`, `minor`, or `patch` manually with
`cargo-release`, even though the commit history contains enough structured
information for most releases.

The Rust ecosystem has several relevant tools:

- Cocogitto provides conventional commit validation, SemVer bump calculation,
  changelog generation, and tag creation.
- release-plz creates release pull requests, can publish crates, and combines
  conventional commits with cargo-semver-checks.
- git-cliff is strong changelog tooling, but it does not own the release
  version and tag decision by itself.

## Decision

Use Cocogitto as the SemVer calculator and keep `cargo-release` as the Cargo
workspace version updater.

The Cocogitto configuration in [[cog.toml]] uses `v` as the root release tag
prefix, reads commits from the latest SemVer tag, ignores merge commits, and
allows automatic bumps only from `main`.

The release command is [[scripts/release-auto.sh]], exposed through
[[mise.toml]] as `mise run release-auto`. The script asks Cocogitto for the next
version with `cog bump --dry-run --auto`, strips any tag prefix, validates the
result as SemVer, updates all Cargo workspace package versions with
`cargo release version`, runs the documented pre-release checks, commits the
version bump, creates both `vX.Y.Z` and `criv-wasm-vX.Y.Z`, and pushes `main`
plus both tags.

Do not let `cog bump` create the release commit directly. Cocogitto's default
bump behavior adds a CI-skip marker to its generated commit, which is risky for
criv because the `v*` tag push is the event that publishes release binaries.
Using `cog bump --dry-run --auto` keeps the SemVer decision in Cocogitto while
leaving commit, tag, and push behavior explicit.

## Consequences

Most releases no longer require a maintainer to choose patch, minor, or major
manually. The release level follows Conventional Commits: `fix` maps to patch,
`feat` maps to minor, and `!` or `BREAKING CHANGE:` maps to major.

The project keeps its existing tag-triggered binary workflow and its current
root plus WASM tag naming scheme. Crates.io publishing remains manual so it can
be decided separately from downloadable CLI binary releases.

The release path now depends on Cocogitto and cargo-release being installed.
Both are pinned through [[mise.toml]], so `mise install` remains the setup entry
point.

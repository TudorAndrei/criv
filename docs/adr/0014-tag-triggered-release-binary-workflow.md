---
id: ADR-0014
kind: decision
title: Tag Triggered Release Binary Workflow
status: accepted
date: 2026-05-14
governs:
  - .github/workflows/release.yml
---

# Tag Triggered Release Binary Workflow

## Context

criv needs downloadable CLI binaries before it can be added cleanly to package
registries such as aqua and mise. The current release process in
[[releasing]] uses `cargo-release` for version changes and tags, but it does
not yet create release artifacts that external installers can consume.

A regular CI build proves the code compiles, but it does not create stable,
versioned assets tied to a public release tag. A manually dispatched release
build would add another release path that could drift from the tag-based
`cargo-release` flow.

## Decision

Build criv release binaries only when a root CLI release tag matching `v*` is
pushed.

The GitHub Actions workflow should build `criv` archives for Linux and macOS on
amd64 and arm64, plus Windows amd64 when practical. Archive names must be
predictable by operating system and architecture so aqua, mise, and direct users
can select the correct asset without custom logic.

Each release should publish checksums beside the archives. GitHub artifact
attestations should be preferred once the workflow is ready for public release
assets. The workflow smoke test is `criv --version`, and the version output must
remain suitable for installer verification.

The workflow should not include a manual dry-run trigger unless a later release
decision adds one. Pre-release validation remains the responsibility of the
local checks documented in [[releasing]].

## Consequences

Release artifacts are created from the same tag event that marks a CLI release,
keeping the binary distribution path aligned with `cargo-release`.

The project avoids a second manual publishing path, but testing the release
matrix before a tag requires local reproduction or a temporary branch-specific
workflow change.

Future aqua and mise registry entries can depend on stable GitHub Release asset
names, checksums, and `criv --version` as the installer smoke test.

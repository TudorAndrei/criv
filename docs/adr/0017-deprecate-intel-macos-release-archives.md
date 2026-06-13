---
id: ADR-0017
kind: decision
title: Deprecate Intel macOS Release Archives
status: accepted
date: 2026-05-15
supersedes:
  - ADR-0014
governs:
  - .github/workflows/release.yml
---

# Deprecate Intel macOS Release Archives

## Context

[[0014-tag-triggered-release-binary-workflow|ADR-0014]] established tag-triggered release binaries for Linux and macOS on
amd64 and arm64, plus Windows amd64 when practical. The Intel macOS
`x86_64-apple-darwin` job runs on the hosted `macos-15-intel` runner and has
become the slowest release job in the matrix, delaying publish completion for
all release assets.

criv's release workflow is primarily meant to provide fast, predictable
installer assets. Apple Silicon macOS remains the dominant current macOS runner
path for the project, and users on Intel macOS can still build from source if
needed.

## Decision

Deprecate the `criv-x86_64-apple-darwin.tar.gz` release archive and remove the
`x86_64-apple-darwin` build from the release workflow.

The release matrix should continue to publish:

- `criv-x86_64-unknown-linux-gnu.tar.gz`
- `criv-aarch64-unknown-linux-gnu.tar.gz`
- `criv-aarch64-apple-darwin.tar.gz`
- `criv-x86_64-pc-windows-msvc.zip`

Reintroduce Intel macOS archives only if there is measured user demand or a
faster runner path that does not materially delay publishing all assets.

## Consequences

Release publishing no longer waits on the slowest hosted macOS Intel job.

Intel macOS users do not receive a first-party downloadable archive and must
build from source unless a later decision restores the target. Installer
registries should not expect `criv-x86_64-apple-darwin.tar.gz` for releases
governed by this decision.

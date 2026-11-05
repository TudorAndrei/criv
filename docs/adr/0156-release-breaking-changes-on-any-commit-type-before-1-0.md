---
id: ADR-0156
kind: decision
title: Release breaking changes on any commit type before 1.0
status: accepted
date: 2026-11-05
governs:
  - scripts/release-auto.sh
---

# Release Breaking Changes on Any Commit Type Before 1.0

## Context

[[0016-conventional-commit-semver-release-automation|ADR-0016]] maps `!` and
`BREAKING CHANGE:` to a release. [[0117-hosted-automatic-release-acceptance|ADR-0117]]
lets Cocogitto select the next version.

Cocogitto 7.0.0 does not do this for every commit type while the version is
`0.y.z`. For `refactor!` or a `refactor` commit with a `BREAKING CHANGE:`
footer, `cog bump --dry-run --auto` reports that no commit requires a bump. A
`feat!` commit still selects a minor version. After `1.0.0`, Cocogitto selects
a major version for a breaking change of any type.

Cocogitto also exits with status 0 when no commit requires a bump. It writes
that message and a hook message to its output. The release script read the last
output line as the version, so the release failed.

## Decision

`scripts/release-auto.sh` reads the version only from a Cocogitto output line
that is a SemVer version. It treats the Cocogitto no-bump message as no release
for every exit status.

When the latest release tag is `v0.y.z` and a non-merge commit in the release
range has `!` after its type or scope or has a `BREAKING CHANGE:` footer, the
script selects the version from `cog bump --dry-run --minor`. Otherwise it
selects the version from `cog bump --dry-run --auto`. A breaking change of any
type therefore makes a minor release before `1.0.0`, also when the range
contains a `fix` commit. Version `1.0.0` remains a manual release.

## Consequences

A breaking `refactor!`, `build!`, or `perf!` commit on `main` makes a release
without a separate `feat` or `fix` commit.

The script now contains one breaking-change pattern of its own. After `1.0.0`,
the script uses only the Cocogitto selection.

## Alternatives Considered

### Add bump rules for every commit type in `cog.toml`

Rejected. A non-breaking `refactor` or `build` commit would then make a release.

### Wait for a `feat` or `fix` commit

Rejected. A released breaking change would stay unpublished for an unknown time.

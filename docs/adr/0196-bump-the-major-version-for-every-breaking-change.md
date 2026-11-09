---
id: ADR-0196
kind: decision
title: Bump the major version for every breaking change
status: accepted
date: 2026-11-09
governs:
  - scripts/release-version.sh
  - scripts/release-auto.sh
---

# Bump the Major Version for Every Breaking Change

## Context

[[0016-conventional-commit-semver-release-automation|ADR-0016]] maps `!` and
`BREAKING CHANGE:` to a major release. Cocogitto 7 does not do that while the
version is `0.y.z`. Its automatic bump never selects `1.0.0`, and it ignores
the breaking marker: `fix!` gives a patch release, and `refactor!` gives no
release at all. The LikeC4 removal (`refactor!`, #224) therefore produced no
release after v0.14.0.

Cocogitto 7 also exits with status 0 when no commit requires a bump, and its
last output line is a hook notice. The release script read that notice as the
version and failed the release run instead of reporting a no-op.

## Decision

`scripts/release-version.sh` owns the next-version decision. It prints the next
version without the tag prefix, or nothing when no commit requires a release.
`scripts/release-auto.sh` and `mise run release-plan` both use it.

A commit since the latest `v*` tag is breaking when its subject has `!` before
the colon, or when a body line starts with `BREAKING CHANGE:` or
`BREAKING-CHANGE:`. Merge commits are ignored, as in `cog.toml`. A squash
commit body counts, so a breaking change inside a squashed pull request still
counts.

When any commit is breaking, the script asks Cocogitto for
`cog bump --dry-run --major`. This applies to `0.y.z` too: a breaking change
after `0.14.0` releases `1.0.0`. Otherwise the script uses
`cog bump --dry-run --auto`, where `fix` gives a patch release and `feat` gives
a minor release. Cocogitto's "no conventional commits required a bump" report
on any exit status is a no-op, not a failure.

## Consequences

The release level follows SemVer for every version, including `0.y.z`. The next
automatic release after v0.14.0 is v1.0.0, because #224 and #229 contain
breaking changes.

A maintainer who does not want a major release must not mark the commit as
breaking. A breaking marker in a squash body releases a major version even when
the squash subject has no `!`.

`tests/hosted_release_scripts.sh` checks the selected version for patch, minor,
no-op, `!`, and `BREAKING CHANGE:` histories on `0.y.z` and `1.y.z` tags.

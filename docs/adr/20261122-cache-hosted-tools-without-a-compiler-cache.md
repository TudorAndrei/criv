---
id: ADR-20261122-cache-hosted-tools-without-a-compiler-cache
kind: decision
title: Cache hosted tools without a compiler cache
status: accepted
date: 2026-11-22
supersedes:
  - ADR-0154
governs:
  - .github/actions/setup-build-tools/**
  - .github/workflows/ci.yml
  - .github/workflows/release.yml
  - .github/workflows/pages.yml
  - tests/ci-cache.sh
  - mise.toml
  - mise.lock
---

# Cache hosted tools without a compiler cache

## Context

[[0154-cache-hosted-toolchains-with-mise-and-compiler-results-with-mbx|ADR-0154]]
put hosted tool installations in a mise cache and compiler results in an mbx
objects cache.

The CI run for commit `3241cc0` took 13 minutes for the core job. The Rust
build and all checks took 59 seconds. The other time went to the caches:

- The mise cache did not hit. mise installed 20 tools in 339 seconds. It
  compiled `mdbook`, `wasm-pack`, and `cargo-release` from source with the
  `cargo:` backend.
- mbx downloaded a 3.3 GB bundle and imported 15.4 GiB. It reused 19 to 629
  compiler actions. Its restore took about 165 seconds, and its save after the
  job took about 180 seconds.

Each mbx bundle was 0.9 to 3.3 GB. Together they filled the 10 GB repository
cache limit, and GitHub removed the mise tool caches. Also, many jobs with the
same tool set tried to save the same mise key at the same time. The save failed
with "Unable to reserve cache".

The local measurement in `docs/tooling.md` also shows that mbx was slower than
plain Cargo with an empty cache.

## Decision

Hosted workflows do not use a compiler-result cache. Remove
`jdx/mr-boxington-action` from the shared setup action. The setup action sets
`MBX_DISABLE=1` and `CARGO_INCREMENTAL=0`, so the mise `cargo` wrapper passes
each command to Cargo. Release jobs compile from clean, as before.

Install `mdbook`, `wasm-pack`, and `cargo-release` from prebuilt release files
through the `aqua:` and `github:` mise backends. Do not use the `cargo:` backend
for a tool that has prebuilt files.

For each tool set and platform, one job saves the mise cache, through the
`save-tool-cache` input. Other jobs with the same key only restore it. Tool
caches publish only from successful pushes to the default branch.

Keep these parts of ADR-0154:

- One shared setup action installs tools for hosted workflows. It puts the mise
  data and `rustup` homes in the runner temporary directory, so the mise cache
  holds the Rust toolchains. The Cargo home stays out of that directory.
- The `cargo-cache` input caches only the Cargo registry and Git downloads.
  Core owns the Linux entry. The Windows and macOS watch jobs own their entries.
  Other jobs restore them.
- Workflows refer to the shared action with the `$/` self-repository syntax.
  `scripts/lint-workflows.sh` changes only those lines for actionlint.
- Developers keep the local mbx `cargo` wrapper and `MBX_TARGET_VIEWS=0`. An
  explicit `MBX_DISABLE` still turns it off.

## Consequences

The tool caches stay inside the repository cache limit, so a job restores its
tools in seconds. Jobs do not spend time on compiler-cache restore and save.

Each hosted Rust job compiles all crates. On a hosted runner, this takes about
one minute. If a hosted build becomes slow, measure it again before adding a
compiler cache.

The prebuilt `mdbook` has no Linux ARM64 file. mise skips it on that platform.
No hosted job builds the book on Linux ARM64.

---
id: ADR-0145
kind: decision
title: Gate Rust dependencies with cargo-deny
status: accepted
date: 2026-10-07
supersedes:
  - ADR-0055
governs:
  - hk.pkl
  - deny.toml
  - mise.toml
  - .github/workflows/release.yml
  - package-lock.json
---

# Gate Rust dependencies with cargo-deny

## Context

[[0055-dependency-auditing-in-hk-checks|ADR-0055]] made the Rust advisory check
monitor-only. `cargo audit --no-fetch` read a dated local advisory database,
and the full check ignored every non-zero exit code. A new RUSTSEC advisory for
a locked crate did not stop a merge or a release. The record in
[[dependency-evaluations]] said that a failing gate needed a reproducible
advisory-database update path first.

`cargo deny check` fetches the current RustSec database on each run. This is
the same posture that ADR-0055 accepted for npm: the hosted advisory feed is a
required dependency of the full check, and an unavailable feed fails the check.
The findings that kept the Rust check monitor-only were in `fff-search`,
`bincode`, and `git2 v0.20.4`.
[[0112-direct-ignore-file-discovery|ADR-0112]] removed all of them.

Release binaries did not record their dependency graph. A scanner could check
`Cargo.lock` at the release tag, but not a downloaded `criv` binary.

## Decision

The full hk check runs these blocking steps:

- `cargo-deny` runs `cargo deny --locked check` with `deny.toml`. It fails for
  a vulnerability, an unmaintained or unsound advisory, or a yanked crate. It
  also fails for a license outside the allow list, a wildcard version, or a
  crate from an unknown registry or Git source. Duplicate crate versions stay
  allowed, as [[0048-2026-07-25-audit-findings-not-actioned|ADR-0048]]
  recorded.
- `obsidian-npm-audit` and `vscode-npm-audit` keep the ADR-0055 policy: `npm
  audit --audit-level=high` fails for a high or critical advisory, or when npm
  cannot get advisory data.

To ignore an advisory, add it to `deny.toml` with a `reason`. The change is
visible in review.

The release workflow builds each `criv` binary with `cargo auditable build`, at
a pinned `cargo-auditable` version. The binary then holds its dependency list,
and `cargo audit bin`, Trivy, Grype, or osv-scanner can scan a downloaded
binary.

`mise.toml` pins `cargo-deny` and no longer installs `cargo-audit`.

Do not add `cargo-vet`. Its audit records and exemption list are a maintenance
cost that this project does not accept. The advisory gate, the license and
source rules, and review of each dependency change are sufficient.

## Consequences

A new RustSec advisory can fail CI on a branch that changed no dependency.
This is deliberate: an advisory must stop merges and releases until a
dependency update or an ignore entry with a reason resolves it.

The gate catches known advisories only. It does not review the code of a new
crate.

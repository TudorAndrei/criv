---
id: ADR-0112
kind: decision
title: Direct ignore file discovery
status: accepted
date: 2026-08-16
supersedes:
  - ADR-0006
  - ADR-0042
governs:
  - Cargo.toml
  - Cargo.lock
  - src/check.rs
  - src/config.rs
  - src/discovery/**/*.rs
  - src/refresh.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/paths.rs
  - src/state.rs
  - src/vault.rs
  - src/watch.rs
  - crates/criv-state-wire/src/lib.rs
  - crates/criv-wasm/src/**/*.rs
  - .obsidian/plugins/criv/src/**/*.ts
  - extensions/vscode-criv/src/**/*.ts
  - fixtures/editor/*.json
  - fixtures/state/*.json
  - scripts/performance/**/*.rs
  - scripts/performance/**/*.sh
  - fixtures/performance/discovery/**/*
  - .github/workflows/ci.yml
  - .github/workflows/discovery-release-gates.yml
  - .github/workflows/release.yml
  - scripts/release-auto.sh
  - scripts/release-publish.sh
---

# Direct Ignore File Discovery

## Context

[[0111-file-discovery-compatibility-contract|ADR-0111]] defines Source, Vault,
and Markdown selection. The old implementation starts one `fff-search` index
for each Source directory root. It also walks Vault files two times and walks
all repository Markdown before it filters a partial changed check.

`fff-search` provides fuzzy ranking, frecency, its own binary classifier, and a
second live watcher. No current criv feature needs fuzzy ranking or frecency.
The extra index makes Source results depend on Git state and prevents one
shared selection contract.

The direct `ignore` crate API provides parallel traversal, Git and `.ignore`
matching, pruning, and traversal errors. It does not report every read error
for ignore-control files. A complete strict implementation would need a fork,
a duplicate preflight, or a custom ignore hierarchy.

## Decision

Use one private `src/discovery/` module. It owns path normalization, traversal,
profile pruning, text selection, link and junction rejection, selected read
errors, duplicate removal, and stable lexical order. It exposes criv values and
does not expose `ignore` types.

Use direct `ignore`, `globset`, and `content_inspector`. Do not add a traversal
backend trait, runtime option, Cargo feature, fallback, direct `walkdir` use,
native code, zlob, or a criv fork of `ignore`.

Keep one separate physical walk and policy for each profile:

- Source uses one combined root plan and one content classifier for directory
  and explicit file roots.
- Vault uses one walk and routes lowercase Markdown and C4 paths to separate
  result lists.
- Markdown uses rumdl include, exclude, and Git-ignore settings. A partial
  `check --changed` evaluates only the staged candidates after ADR-0067 keeps
  the transaction partial.

For ignore-control files only, accept the native `ignore` error behavior. The
crate can suppress some I/O errors while it reads `.gitignore`, `.ignore`,
global Git ignore, and `.git/info/exclude`. This is a narrow exception to the
strict selected-scope error rule in ADR-0111. Traversal errors, invalid criv or
rumdl patterns, selected links or junctions, invalid UTF-8 identities, and
selected-file read errors still fail. Do not add a duplicate read or traversal
to hide this library limit.

Use the repository `notify` watcher as the only live watcher. A relevant Source
event marks Source dirty and runs the same authoritative selector as a one-shot
command. Content events rescan even when the selected path list does not
change. Unknown or watcher errors suspend the generation, recreate the
watcher, and complete a full scan before publication. A failed scan keeps the
last successful State and retries after the next relevant event.

Keep Source graph content hashing outside discovery. Keep exact, suffix, and
basename Source lookup, explicit ambiguity, and stable lexical choice. Remove
fuzzy ranking and frecency. Remove frecency from Rust and editor types, State
output, Wasm behavior, active fixtures, and performance schemas. Keep
`criv.state.v1`; new readers accept an older State document with the unknown
extra field.

Remove `fff-search` after every production caller uses the new module. Do not
increase the normal dependency count.

## Compatibility and Evidence

Run one logical contract corpus on Linux, macOS, and Windows. It covers profile
case rules, hidden and ignored files, missing and overlapping roots, explicit
files, invalid patterns, links and junctions, unreadable selected paths, pruned
errors, stable order, and non-UTF-8 names where the platform can create them.

Lock binary behavior for empty, ASCII, UTF-8, invalid UTF-8 without NUL,
BOM-marked UTF-8, UTF-16, and UTF-32, the first 1,024-byte NUL boundary, PDF and
PNG prefixes, generated text, extension independence, root-form parity, and
the known PGM and protobuf false negatives.

Keep performance observation outside production code as required by
[[0072-keep-performance-observation-outside-core|ADR-0072]]. Use a release-mode
test-only selector probe for component evidence and the ordinary release binary
for command evidence. Record five successful samples, raw failed rows,
min/median/max/MAD, output identities, per-child peak RSS, live readiness and
convergence, the exact workload and machine, and all binary digests.

The pure-Rust replacement must pass these hard limits:

- Ouro command elapsed time and peak RSS are at most 110 percent of the matched
  v0.9.0 result. The accepted absolute command medians are 1.0483 seconds cold,
  0.8041 seconds warm, 0.6259 seconds changed Source, and 0.6314 seconds changed
  Markdown. Peak RSS limits are 277.6, 274.5, 277.9, and 276.7 MB.
- Five of five live samples publish State, reach readiness, complete create,
  rename, and delete, and match one-shot State. Readiness median is at most 1.25
  seconds. Every readiness and convergence sample is at most 2 seconds.
- Source elapsed time is at most 110 percent at 9,000 selected files and at
  most 50 percent at 90,000 and 225,000 files. The accepted absolute medians
  are 0.2673, 1.1875, and 10.3725 seconds.
- Vault and Markdown scaling time and all scaling peak RSS values are at most
  110 percent of baseline. The gate verifier owns the accepted absolute values
  from issue 130.
- The stripped binary does not grow on any release target. Three clean build
  medians are at most 110 percent of baseline. All release targets build. The
  change adds no native compiler, bindgen, libclang, native library, or normal
  dependency.

Run the matched 100,000-entry Source workload on all four native release
platforms. Run the full Ouro evidence and 250,000-entry workloads on the
controlled primary host. A result with more than 10 percent relative MAD gets
one complete repeat. A second unstable attempt is not gate evidence.

Ordinary push performance notes stay non-blocking. Release acceptance writes a
short-lived receipt to `refs/notes/criv-release-gates`. The receipt records the
commit, toolchain, workloads, machines, baseline and candidate binaries, raw
evidence digests, artifact digests, and every gate result. The tag workflow
publishes only the four measured binaries named by a valid receipt for the
exact tag commit.

## Consequences

Source discovery no longer owns a database, fuzzy scorer, frecency store, or
second watcher. Git state does not change Source selection. Vault selection
uses one traversal. Partial changed checks do not pay for a repository-wide
Markdown walk.

The release process has two steps. `release-auto` creates and pushes the version
commit. Controlled acceptance measures that exact commit and publishes its
receipt. `release-publish` creates tags only after the receipt passes.

An active ignore-control file can still have an I/O error that `ignore` does
not report. This accepted library limit is visible and narrow. Revisit it only
if upstream adds a suitable released API or a real defect justifies the added
implementation cost.

Before release, any hard-gate failure blocks the tag. After release, a
correctness or live-convergence defect requires a revert or patch release. A
timing or RSS rollback needs two matched stable attempts that show more than a
20 percent regression. There is no runtime `fff-search` fallback.

## Alternatives Considered

### Keep fff-search for Source

Rejected. It keeps two file-selection systems, two watchers, two binary rules,
and unused ranking state.

### Fork ignore or add a strict preflight

Rejected. A fork adds maintenance and supply-chain work. A preflight duplicates
ignore-file reads and has a time-of-check/time-of-use gap.

### Add a backend interface for a future native implementation

Rejected. There is one selected backend. A future native backend must first
beat direct `ignore` by the separate issue 130 gates. Synthetic speed alone is
not enough.

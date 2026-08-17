---
id: ADR-0117
kind: decision
title: Hosted automatic release acceptance
status: accepted
date: 2026-08-17
supersedes:
  - ADR-0116
governs:
  - .github/workflows/ci.yml
  - .github/workflows/release.yml
  - scripts/release-auto.sh
  - scripts/package-release-assets.sh
  - scripts/performance/assemble-hosted-release-gates.sh
  - scripts/performance/publish-release-gate-note.sh
  - scripts/performance/src/bin/criv-discovery-gate.rs
  - tests/hosted_release_scripts.sh
  - tests/performance_release_gate_note.sh
  - mise.toml
---

# Hosted Automatic Release Acceptance

## Context

[[0116-run-release-acceptance-on-the-controlled-local-computer|ADR-0116]]
requires a controlled local Mac for final file-discovery acceptance and release
publication. The computer will not be a self-hosted GitHub Actions runner. The
local evidence transfer and publication steps are slow, difficult to repeat,
and not suitable for automatic releases.

Ouro was useful as one observed large repository during file-discovery design.
It is not a stable hosted workload. Its ignored build trees are local and can
change independently from its Git commit. Requiring this checkout for every
release makes one private computer a release service.

Barrs shows the required Cocogitto release pattern. Criv needs a stronger
transaction because it publishes four measured binaries and a release-gate
receipt. A commit, tag, or release made with `GITHUB_TOKEN` does not start the
normal follow-up push or release workflows. The release workflow must own the
complete transaction.

## Decision

Run release acceptance and publication on GitHub-hosted runners. Do not use a
self-hosted runner. Keep Ouro as an optional manual benchmark. Ouro results do
not approve or block a release.

Start the release workflow only after the normal CI workflow passes for a push
to `main` from this repository. A stale run exits when `main` has moved. The
workflow uses one non-cancelling concurrency group. It does not use force push.
Commits that arrive after a release starts remain for the next release.

Use Cocogitto and Conventional Commits to select the next version. A range with
no release change is a successful no-op. The workflow updates all workspace
Cargo versions and pushes one `chore(release): vX.Y.Z` commit. It then runs the
complete Rust, vault, Obsidian, VS Code, Wasm, and Actions checks again on that
exact commit. The repository does not keep a generated changelog. GitHub
generates the release notes.

Use v0.9.0 as the first matched performance baseline. Later releases compare
with the last stable release that has a compatible evidence contract. A new
behavior contract needs an accepted ADR before it can select a new baseline.
Run baseline and candidate samples on the same hosted runner.

Run the 100,000-entry Source workload on Linux x86_64, Linux ARM64, macOS ARM64,
and Windows x86_64. Run the 250,000-entry Source, Vault, and Markdown workloads
on macOS ARM64. The selected file counts are 90,000 and 225,000 because each
workload includes directory entries. Run five live-watch samples on a generated
macOS workload. Each sample must publish State, reach readiness, complete the
create, rename, and delete sequence, and match one-shot State.

Keep the accepted matched ratio, peak memory, binary size, clean build,
dependency, toolchain, output identity, and five-sample stability gates. Remove
the local-computer absolute time and memory limits. They do not describe a
GitHub-hosted machine. Keep the 50 percent Source time ratio for the large
matched workloads and the 110 percent no-regression limits.

Build each native release binary during its three-sample clean-build job. The
gate receipt records that exact binary. Packaging must use the same bytes and
must not rebuild the CLI. Keep raw measured binaries and evidence as Actions
artifacts for 90 days. Store the receipt, archives, checksums, and attestations
with the GitHub release without a time limit.

The passing receipt is valid for seven days and is published to
`refs/notes/criv-release-gates` before tags. Package and check the four measured
binaries with the VS Code package. Verify each archive on its native hosted
runner. Then push `vX.Y.Z` and `criv-wasm-vX.Y.Z` in one atomic operation,
create or resume a draft release, upload and attest every asset, verify the
draft, and publish it.

A failure before tags leaves the prepared version commit and no release. A
manual workflow run can retry that exact commit and tag. A later automatic run
resumes an untagged prepared release instead of making a second version bump.
A matching draft is resumable. Existing matching tags are valid. A tag that
points to another commit stops the workflow. A matching published release is
complete, and no workflow can move its tags.

## Consequences

A contributor pushes normal Conventional Commits to `main`. CI and the release
workflow do the remaining work. The controlled local computer has no required
release role.

The Windows evidence lane can take more than one hour. Release publication is
therefore slower than a normal CI run. It is repeatable and does not need a
developer to remain available.

The privileged workflow uses `workflow_run`, but accepts only a successful
`push` run on `main` from this repository. It checks out trusted repository
commits and does not consume artifacts from the CI run. Write permissions are
limited to the prepare, receipt, and publication jobs.

The release remains tag-only. This decision does not publish crates to
crates.io.

## Alternatives Considered

### Keep controlled local acceptance

Rejected. It makes one developer computer part of every release and requires a
manual evidence transfer.

### Register a self-hosted runner

Rejected. The computer is not a GitHub job service.

### Keep Ouro as a hard release gate

Rejected. Its large ignored trees are not fixed by its Git revision and are not
available on hosted runners. Synthetic workloads provide repeatable scaling
evidence. Ouro remains useful for optional product observation.

### Copy the Barrs push-then-dispatch flow

Rejected. A dispatch failure can leave a release commit and tags without a
release run. Criv keeps preparation, evidence, verification, tags, and
publication in one explicit workflow.

### Verify only after publication

Rejected. A failed native check would leave a published defective release. The
workflow verifies the candidate archives before publication.

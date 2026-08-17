---
id: ADR-0116
kind: decision
title: Run release acceptance on the controlled local computer
status: accepted
date: 2026-08-17
supersedes:
  - ADR-0115
governs:
  - Cargo.toml
  - Cargo.lock
  - src/check.rs
  - src/config.rs
  - src/discovery/**/*.rs
  - src/lib.rs
  - src/refresh.rs
  - src/source.rs
  - src/source/catalog.rs
  - src/source/graph.rs
  - src/source/paths.rs
  - src/state.rs
  - src/vault.rs
  - src/watch.rs
  - scripts/performance/discovery_probe.rs
  - scripts/performance/adapters/*.patch
  - scripts/performance/accept-release-gates.sh
  - scripts/performance/publish-release-gate-note.sh
  - scripts/performance/src/bin/criv-discovery-baseline.rs
  - scripts/performance/src/bin/criv-discovery-gate.rs
  - fixtures/performance/discovery/**/*
  - .github/workflows/ci.yml
  - scripts/release-auto.sh
  - scripts/release-publish.sh
  - .github/workflows/discovery-remote-evidence.yml
  - .github/workflows/release.yml
  - mise.toml
---

# Run Release Acceptance on the Controlled Local Computer

## Context

[[0115-single-read-source-build|ADR-0115]] requires the full Ouro and macOS
acceptance run on one controlled computer. The first delivery code incorrectly
made that computer a GitHub Actions self-hosted runner. The project will not
register the computer as a GitHub runner.

GitHub does not provide a supported command that uploads a normal Actions
artifact from a local process. The release flow must therefore move accepted
assets from the local computer to GitHub without a self-hosted workflow.

## Decision

Retain all file-discovery behavior, Source pipeline behavior, performance
limits, evidence rules, and rollback rules from ADR-0115. Change only the
acceptance and release delivery path.

Run the final gate verifier with a local command on the controlled macOS ARM
computer. The command accepts one prepared evidence bundle for the exact
candidate commit. It copies the four measured binaries to the ignored
`.criv/release-gates/<commit>/` directory. It writes the passing, seven-day
receipt to `refs/notes/criv-release-gates`.

Do not register the controlled computer as a GitHub Actions runner. Hosted
Linux and Windows workflows can produce remote evidence and measured binaries,
but they cannot accept the release.

After acceptance, run the publish command on the same local computer. It must
match the local receipt to the Git note and verify the SHA-256 value of each
accepted binary. It packages these exact binaries with the editor viewer. It
creates the two release tags, uploads a draft GitHub release, and publishes the
release only after all assets are present. It does not build replacement CLI
binaries.

The GitHub release workflow starts after the release is published. Each native
host verifies its archive checksum, accepted binary digest, and version. A
final hosted job adds build-provenance attestations to the published assets.
The workflow does not replace or upload release assets.

## Consequences

The private computer stays private and has no GitHub job service. Local raw
evidence and accepted binaries stay outside source control. The Git note keeps
the short receipt and stable digests.

Release publication now needs the local accepted asset directory. A different
computer can publish only if the operator securely transfers that directory
and verifies that it matches the receipt.

The release assets exist before hosted native verification and attestation
complete. A failure is visible as a failed release workflow and requires a
corrected patch release. If local upload fails, the release stays as a draft
and can be repaired or removed before publication.

## Alternatives Considered

### Register the computer as a self-hosted runner

Rejected. It exposes the computer to GitHub job execution and was not part of
the accepted operating model.

### Upload an Actions artifact from the local command

Rejected. GitHub supports Actions artifact upload inside a workflow. It does
not provide a supported local upload path.

### Add a second artifact registry

Rejected. An OCI or object-storage staging service adds credentials, tools,
retention rules, and another release dependency. The local publish command can
upload the final release assets directly.

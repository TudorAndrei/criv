---
id: ADR-0132
kind: decision
title: Use Aqua With a Mise Alias and GitHub Fallback
status: accepted
date: 2026-08-21
governs:
  - README.md
  - .github/workflows/release.yml
---

# Use Aqua With a Mise Alias and GitHub Fallback

## Context

The current mise setup uses the direct `github:TudorAndrei/criv` backend. This
works, but users must know the repository name. The release workflow already
publishes platform archives, SHA-256 checksums, a release manifest, and GitHub
artifact attestations.

[[installer-registry-distribution]] compares direct GitHub installation, a
mise registry record, an aqua Standard Registry record, and new installer
plugins. It also records the current release targets, asset names, integrity
flow, registry ownership, update duties, and failure modes.

The installer route must lower setup cost without creating a second installer
codebase or claiming platforms that criv does not release.

## Decision

Use the aqua Standard Registry as the main shared package record. After that
record is merged and available in a released registry version, request the
`criv` short name in the mise registry.

The mise record uses these backends in order:

```toml
backends = ["aqua:TudorAndrei/criv", "github:TudorAndrei/criv"]
bins = ["criv"]
description = "Keep repository documentation connected to the code it describes"
test = { cmd = "criv --version", expected = "criv {{version}}" }
version_order = "semver"
```

Keep `github:TudorAndrei/criv` as the direct, authoritative fallback and the
bootstrap path before registry changes are available. Do not create an asdf
plugin, a vfox plugin, a custom mise backend, or a private aqua registry. If
mise rejects the short name, keep the aqua and direct GitHub forms. Do not use
a plugin as a fallback.

### Aqua package contract

Generate the initial aqua package with the aqua registry `argd s` workflow.
Review and test the generated record. Do not hand-write the initial record.

The current package supports only these release environments:

- Linux AMD64 with glibc;
- Linux ARM64 with glibc;
- macOS ARM64; and
- Windows AMD64.

Map these environments to the existing `criv-<rust-target>` archives. Windows
uses ZIP. The other environments use `tar.gz`. The package exposes only
`criv` or `criv.exe`.

Read checksums from `SHA256SUMS.txt` and require
`.github/workflows/release.yml` as the GitHub artifact attestation signer.
Keep a version override that permits macOS AMD64 only through `v0.2.0`, where
that asset exists. Do not claim macOS AMD64 for current releases.

Do not use the abandoned aqua immutable-release option. A later decision can
enable GitHub immutable releases if the release process adopts them.

### Publication order

Use this order:

1. Add and test `TudorAndrei/criv` in `aquaproj/aqua-registry`.
2. Wait until the package is available in a released Standard Registry
   version.
3. Add and test `registry/criv.toml` in `jdx/mise` with aqua first and GitHub
   second.
4. Change criv installation documents to use `criv = "latest"` as the main
   mise example only after the mise record is available.
5. Keep the direct GitHub and full aqua forms in the documents as fallbacks.

No new criv release is required unless registry tests find a release-package
defect.

### Maintenance

Normal new versions do not require a registry edit because both selected
backends discover GitHub Releases. Update the external records before a
release changes any of these facts:

- the root `vX.Y.Z` tag pattern;
- the repository owner or name;
- an archive name, format, or executable path;
- a supported operating system or CPU;
- the checksum file or line format;
- the attesting workflow path; or
- the `criv --version` output.

Keep the current archive verification, version smoke tests, checksums,
manifest, and artifact attestations for every release. Do not change registry
metadata to hide a release-package defect.

## Consequences

Aqua users get a maintained package record. Mise users can use the short
`criv` name if the mise maintainers accept it. Direct GitHub installation
continues to work when a registry is unavailable or delayed.

Criv does not own new installer code or a new release stream. It does own the
duty to keep external registry metadata aligned with its release contract.
The external maintainers can reject or delay an entry, so this decision keeps
all documented fallback forms.

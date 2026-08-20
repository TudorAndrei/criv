---
id: installer-registry-distribution
kind: doc
title: Installer registry distribution
---

# Installer registry distribution

Research date: 2026-08-21.

## Conclusion

Use the aqua Standard Registry as the main shared package record. Then add a
`criv` short name to the mise registry. The mise record must use aqua first and
the direct GitHub backend second:

```toml
backends = ["aqua:TudorAndrei/criv", "github:TudorAndrei/criv"]
bins = ["criv"]
description = "Keep repository documentation connected to the code it describes"
test = { cmd = "criv --version", expected = "criv {{version}}" }
version_order = "semver"
```

Keep `github:TudorAndrei/criv` as the direct and authoritative fallback. Do not
make an asdf plugin, a vfox plugin, a custom mise backend, or a private aqua
registry.

This route gives these user paths after the external registry changes merge:

```toml
[tools]
criv = "latest"
```

```yaml
packages:
  - name: TudorAndrei/criv@v0.10.1
```

The first form is the lowest-cost mise setup. The second form is the native
aqua setup. The present direct GitHub form continues to work before and after
the registry work:

```toml
[tools."github:TudorAndrei/criv"]
version = "latest"
```

The mise registry can reject a tool that it does not find notable. Its current
contribution guide says that a new tool can be rejected for this reason. Thus,
the mise short name is a request, not a result that criv controls. An aqua entry
still gives a maintained package record for aqua and for `mise` through the
full `aqua:TudorAndrei/criv` name. Do not make a plugin if the mise short-name
request is rejected. [The mise contribution guide states both the notability
rule and the accepted backend tiers.](https://mise.jdx.dev/contributing.html#adding-tools)

## Present release contract

The [README](../../README.md) already uses the direct mise GitHub backend. The
[release guide](../releasing.md) and the
[release workflow](../../.github/workflows/release.yml) define four native
archives, one checksum file, one release manifest, and GitHub build provenance
attestations.

The current release is `v0.10.1`. Its official GitHub release record has these
supported targets:

| User platform | Rust target | Release asset |
| --- | --- | --- |
| Linux x86-64 with glibc | `x86_64-unknown-linux-gnu` | `criv-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 with glibc | `aarch64-unknown-linux-gnu` | `criv-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `criv-aarch64-apple-darwin.tar.gz` |
| Windows x86-64 | `x86_64-pc-windows-msvc` | `criv-x86_64-pc-windows-msvc.zip` |

The same record has `SHA256SUMS.txt` and `release-manifest.json`. It reports a
SHA-256 digest for each release asset. It also reports `immutable: false` for
the release. [GitHub release record for
`v0.10.1`](https://api.github.com/repos/TudorAndrei/criv/releases/373822754).

The release workflow attests all four archives, the checksum file, and the
manifest. GitHub states that artifact attestations connect an artifact to its
source and build instructions. It also states that verification is necessary
to get this benefit. [GitHub artifact attestation
model](https://docs.github.com/en/actions/concepts/security/artifact-attestations).

An isolated check with mise 2026.8.10 installed `v0.10.1` on macOS ARM64 from
the direct GitHub backend. Mise selected the Apple Silicon archive, calculated
its checksum, verified its GitHub artifact attestation, extracted it, and got
`criv 0.10.1` from `criv --version`. No release-layout change is necessary for
the direct path.

The current target list is an intentional limit. The registry data must not
claim support for Linux musl, macOS Intel, Windows ARM64, or another target
that the current release does not contain. Historical releases `v0.1.1` and
`v0.2.0` also contain `criv-x86_64-apple-darwin.tar.gz`. The aqua entry must
keep this old macOS Intel support in a version override. It must not claim that
the current release has this target. [GitHub release record for
`v0.2.0`](https://api.github.com/repos/TudorAndrei/criv/releases/tags/v0.2.0).

## Option comparison

| Path | Setup cost | Version and platform selection | Integrity | Metadata owner and update duty | Main failure modes | Result |
| --- | --- | --- | --- | --- | --- | --- |
| Direct GitHub asset or `github:TudorAndrei/criv` | Low, but users must know the repository name. A manual download also needs PATH setup. | GitHub Releases owns the version list. Mise strips the normal `v` tag prefix, excludes prereleases by default, and scores release assets by operating system, CPU, C library, and archive type. [Mise GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html) | Current mise verifies GitHub artifact attestations by default when they exist. A mise lock file can store checksums and verified provenance. [GitHub backend attestation setting](https://mise.jdx.dev/dev-tools/backends/github.html#github_attestations), [mise lock file](https://mise.jdx.dev/dev-tools/mise-lock.html) | criv owns only its release assets and tags. Each user owns any special backend options. No central package record exists. | The name is long. An asset-name change can change automatic selection. GitHub API limits or an attestation-service failure can stop a first install. The current release is not immutable. | Keep as the fallback and bootstrap path. |
| Mise registry entry with the GitHub backend only | Very low: `mise use criv`. | The mise record supplies the short name and test. The GitHub backend still owns version discovery and asset selection. | It has the same attestation and lock-file behavior as the direct GitHub form. | The mise repository owns `registry/criv.toml`. criv must update it if the command, repository, release scheme, or backend changes. No per-version edit is necessary while the contract stays stable. | The mise maintainer can reject the tool for low notability. Registry data in a mise release can lag the source registry. It gives no native aqua entry. | Useful, but incomplete by itself. |
| New asdf or vfox plugin | High. A plugin adds executable install logic, releases, tests, and a separate trust boundary. | The plugin must implement version listing, download selection, and installation. | Security depends on plugin ownership and plugin code. | criv or a third party must maintain a second software project. | The mise registry does not accept new asdf or vfox entries. The plugin can drift from the release workflow. [Mise backend acceptance policy](https://mise.jdx.dev/contributing.html#backend-acceptance-tiers) | Reject. |
| Aqua Standard Registry entry | Low for aqua users and low for mise users through `aqua:`. | Aqua gets the latest version from GitHub Releases by default. The package record maps the exact four supported environments to the four assets. [Aqua version source](https://aquaproj.github.io/docs/reference/registry-config/version-source/), [supported environments](https://aquaproj.github.io/docs/reference/registry-config/supported-envs/) | The record can read `SHA256SUMS.txt` and require the release workflow as the GitHub attestation signer. Aqua supports this attestation field from version 2.35.0. Mise uses the aqua record with checksum verification and native attestation verification. [Aqua checksum record](https://aquaproj.github.io/docs/reference/registry-config/checksum/), [Aqua GitHub attestations](https://aquaproj.github.io/docs/reference/registry-config/github-artifact-attestations/), [mise aqua security](https://mise.jdx.dev/dev-tools/backends/aqua.html#security-verification) | The aqua registry owns `pkgs/TudorAndrei/criv/registry.yaml`, its test data, and the merged registry. criv must update the entry only when its release contract changes. | A registry snapshot can lag. Direct aqua checksum enforcement depends on the user's checksum settings. Unsupported platforms must fail clearly. An attestation API failure can stop installation. | Select as the main shared record. |
| Private aqua registry | Higher than the Standard Registry. Users must add and trust another registry source. | criv would own registry publication and versioning. | It can use the same checksum and attestation fields, but users must approve the non-standard registry. | criv must run and release the registry. | More setup, one more release stream, and one more availability dependency. Aqua recommends the Standard Registry for public tools. [Aqua custom-registry guidance](https://aquaproj.github.io/docs/develop-registry/) | Reject. |

## Integrity flow

The release workflow already has the necessary producer flow:

1. It builds each target from one release commit.
2. It packages the binary and the editor archive.
3. It writes SHA-256 values to `SHA256SUMS.txt` and
   `release-manifest.json`.
4. It verifies the archives on their target systems.
5. It attests the archives, checksum file, and manifest with
   `.github/workflows/release.yml`.
6. It publishes the GitHub release.

The aqua record must connect to that flow with this data after `argd s`
generates the package files:

```yaml
asset: criv-{{.Arch}}-{{.OS}}.{{.Format}}
format: tar.gz
replacements:
  amd64: x86_64
  arm64: aarch64
  darwin: apple-darwin
  linux: unknown-linux-gnu
  windows: pc-windows-msvc
overrides:
  - goos: windows
    format: zip
supported_envs:
  - linux/amd64
  - linux/arm64
  - darwin/arm64
  - windows/amd64
checksum:
  type: github_release
  asset: SHA256SUMS.txt
  file_format: regexp
  algorithm: sha256
  pattern:
    checksum: ^(\b[A-Fa-f0-9]{64}\b)
    file: "^\\b[A-Fa-f0-9]{64}\\b\\s+(\\S+)$"
github_artifact_attestations:
  signer_workflow: TudorAndrei/criv/.github/workflows/release.yml
```

The generated asset mapping must produce these target triples:

- `amd64` becomes `x86_64`.
- `arm64` becomes `aarch64`.
- Linux uses `unknown-linux-gnu` and `tar.gz`.
- macOS uses `apple-darwin` and `tar.gz`.
- Windows uses `pc-windows-msvc` and `zip`.

The generated record must also have a `version_overrides` item through
`v0.2.0`. That item must add `darwin/amd64` to `supported_envs`. The final
override for current versions must use only the four environments shown above.
This keeps old installations valid without restoring macOS Intel for new
releases.

The `SHA256SUMS.txt` format has two spaces between the digest and asset name.
The regular expressions above also accept other whitespace. This agrees with
the aqua checksum parser contract.

Do not add `github_immutable_release` to the aqua record. Aqua abandoned this
feature in version 2.60.1, and the current criv release is not immutable.
[Aqua security feature status](https://aquaproj.github.io/docs/reference/security/).
GitHub has a separate release-integrity command for repositories that enable
immutable releases. This can be a later release hardening task, but it is not a
registry prerequisite. [GitHub release integrity
verification](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity).

For direct aqua use, checksum verification is disabled in user configuration
by default. Aqua recommends `enabled: true` and `require_checksum: true`, and it
recommends that users commit `aqua-checksums.json`. GitHub artifact attestation
verification has an explicit disable flag and runs when the package record has
the signer field. [Aqua checksum user
settings](https://aquaproj.github.io/docs/reference/config/checksum/), [aqua
configuration](https://aquaproj.github.io/docs/reference/config/).

For mise use, enable `mise.lock` in projects that require repeatable installs.
The lock file stores the exact version, URL, checksum, size, and available
provenance. Strict lock mode can prevent a new GitHub or registry lookup during
installation. [Mise lock-file contract](https://mise.jdx.dev/dev-tools/mise-lock.html).

## Exact implementation route

### 1. Add the aqua package

1. Fork `aquaproj/aqua-registry`.
2. Run `argd s TudorAndrei/criv`. Do not hand-write the first package record.
   The Standard Registry requires this command for a new GitHub Release
   package. It reads the GitHub releases and assets, creates
   `pkgs/TudorAndrei/criv/registry.yaml` and
   `pkgs/TudorAndrei/criv/pkg.yaml`, creates a branch and commit, and runs its
   container tests. [Official add-package
   process](https://github.com/aquaproj/aqua-registry/blob/main/docs/add_package.md).
3. Review all generated versions and asset rules. The repository has both a
   root CLI tag and a WASM tag, but only root CLI GitHub Releases are valid for
   this package. Do not use an asset allow list unless the generator includes
   unrelated assets. An allow list can remove `SHA256SUMS.txt` by mistake.
4. Add the four exact `supported_envs`, the `SHA256SUMS.txt` parser, and the
   `github_artifact_attestations.signer_workflow` value shown above if the
   generator does not add them.
5. Add a version override that supports `darwin/amd64` through `v0.2.0`. Do
   not add macOS Intel to the final override for current versions.
6. Confirm that the package exposes only `criv` or `criv.exe`. The bundled
   `vscode-criv.vsix` is data for `criv install-editor`; it is not another
   command.
7. Run `argd t TudorAndrei/criv`, then run `argd gr` to update the merged root
   registry. Test the latest release on the four
   supported environments. Keep one old version in `pkg.yaml` only if an old
   asset rule needs a version override. The aqua registry states that
   `pkg.yaml` is test data, not the list of installable versions. [Registry
   structure](https://github.com/aquaproj/aqua-registry/blob/main/docs/structure.md),
   [`pkg.yaml` guide](https://github.com/aquaproj/aqua-registry/blob/main/docs/pkg_yaml.md).
8. Open the aqua-registry pull request. The Standard Registry covers Windows,
   macOS, and Linux on AMD64 and ARM64. The final criv rule must use only the
   four targets in the current release. [Aqua registry support
   policy](https://github.com/aquaproj/aqua-registry/blob/main/docs/support_policy.md).

No new criv release is necessary unless the registry tests find a packaging
defect.

### 2. Add the mise short name

After the aqua package is in a released Standard Registry version:

1. Add `registry/criv.toml` to `jdx/mise` with the exact TOML record in the
   conclusion.
2. Run `mise test-tool criv` on the pull request. The mise registry requires a
   reliable test with `{{version}}`. It accepts aqua and GitHub as preferred
   backends and uses the first available backend. [Mise tool-entry
   requirements](https://mise.jdx.dev/contributing.html#guidelines-and-requirements),
   [backend priority](https://mise.jdx.dev/contributing.html#backend-priority).
3. In the pull request, give the current release date, release cadence,
   download count, and repository use evidence. The mise maintainer requests a
   short popularity summary for notability review.
4. If mise rejects the short name, stop. Keep the aqua and direct GitHub forms.
   Do not replace them with a plugin.

By default, mise uses the aqua registry snapshot that is built into its own
release. Users can opt in to `registry_floating` to check the current official
aqua registry and the current mise shorthand registry first. A normal new mise
release removes this lag without a criv change. [Mise aqua registry
behavior](https://mise.jdx.dev/dev-tools/backends/aqua.html), [mise registry
behavior](https://mise.jdx.dev/registry).

### 3. Change criv documentation after acceptance

Only after the mise entry is available, change the README primary example to
`criv = "latest"`. Keep `github:TudorAndrei/criv` as a documented fallback.
Add the aqua package form to the release or installation guide. Do not promise
an unsupported platform.

### 4. Maintain the entries

No registry edit is necessary for a normal new version. Both backends discover
versions from GitHub Releases. The aqua registry updater checks for package
updates and can open update pull requests. Aqua maintainers review and merge
those changes. [Standard Registry update
workflow](https://github.com/aquaproj/aqua-registry/blob/main/.github/workflows/update.yaml).
Update the external records before the first release that changes one of these
facts:

- the `vX.Y.Z` root tag pattern;
- the repository owner or name;
- an archive name, format, or root executable;
- a supported operating system or CPU;
- the checksum file name or line format;
- the attesting workflow path;
- the `criv --version` output.

For each release, keep the present release checks: archive verification,
`criv --version`, `SHA256SUMS.txt`, the manifest, and GitHub artifact
attestations. If one of these checks fails, do not change registry metadata to
hide the release defect.

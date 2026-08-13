---
id: ADR-0103
kind: decision
title: Required Repository Self Governance
status: accepted
date: 2026-08-13
governs:
  - criv.toml
  - .gitignore
  - Cargo.toml
  - crates/**/Cargo.toml
  - scripts/performance/**/Cargo.toml
  - mise.toml
  - mise.lock
  - package.json
  - package-lock.json
  - packages/**/package.json
  - .obsidian/plugins/criv/package.json
  - extensions/vscode-criv/package.json
  - .github/workflows/**
  - hk.pkl
  - tests/performance_harness.sh
  - tests/performance_git_note.sh
  - scripts/performance/measure-state-wasm.test.mjs
  - scripts/prepare-docs-site.mjs
  - scripts/release-auto.sh
  - extensions/vscode-criv/README.md
---

# Required Repository Self Governance

## Context

The repository validates itself as a criv vault. The source catalog did not
contain all maintained inputs. A new crate, package, or extension could avoid
the catalog when only selected child directories were listed. Some site and
performance tooling changes did not have a required pull request check.

The Rust version in the release workflow differed from the development and CI
version. The repository did not declare an MSRV. The Pages workflow selected
the latest mdBook release at run time. The two editor host suites also ran the
same shared package tests, but no CI lane owned those tests directly.

[[0072-keep-performance-observation-outside-core|ADR-0072]] keeps measured
timings outside correctness gates. It permits deterministic tests of the
performance tools. [[0084-require-windows-hosted-validation|ADR-0084]] and the
existing CI decisions require hosted evidence on supported systems.
[[0087-keep-editor-setup-out-of-init|ADR-0087]] keeps editor recommendation and
installation out of `criv init`.

## Decision

### Self-vault catalog

The source catalog contains every maintained text input that can change
repository behavior or validation. It uses the parent roots `crates`,
`packages`, and `extensions`. It also contains `src`, `tests`, `scripts`,
`fixtures`, `site`, `.github`, `.config`, `.vscode`,
`.obsidian/plugins/criv`, `assets`, `.agents/skills`, and the maintained
top-level manifests, lock files, configuration files, `AGENTS.md`, `README.md`,
and `LICENSE`.

The `docs` directory remains the documentation vault. It is not also a source
root. Dependency directories, build outputs, local `.criv` State, research
artifacts, the `.claude` skill link, generated editor packages, and the
generated site architecture are not source inputs.

A catalog change must use a successor ADR. The source catalog itself is the
configuration input to criv. No second file repeats its full contents as a
test fixture.

### Exact tool contract

Rust `1.97.1` is both the repository toolchain and the MSRV. Every Cargo
workspace package declares this version. Mise, CI, Wasm builds, performance
tools, and release builds use it. Automated Cargo build, test, and run commands
use the committed lock file with `--locked`.

mdBook `0.5.4` is a mise tool. No workflow selects a tool with `latest`. Direct
npm dependency entries use exact versions. End-user installer examples and the
VS Code host compatibility range are not repository build-tool selections and
can keep their compatibility syntax.

### Required validation surface

The CI job named `Repository checks` is the only stable branch protection
result. It fails unless every correctness lane succeeds. It includes the core,
Wasm, shared editor contract, Obsidian, VS Code, Windows, macOS watch-lock,
site, and performance-tooling smoke lanes.

One change classifier selects the site and performance smoke work. Each
selected lane still reports success when its inputs did not change. Therefore,
the aggregate result does not change its name or dependency set for a narrow
change.

The site lane runs for changes to documentation, architecture, site inputs,
the site preparation script, its exact tools, or its workflows. The root
`npm run build:site` command prepares the documentation, builds the full
language-independent LikeC4 workspace, and builds the mdBook. The Pages
deployment calls this same command.

The performance smoke lane runs these deterministic correctness tests:

- `bash tests/performance_harness.sh`
- `bash tests/performance_git_note.sh`
- `node --test scripts/performance/measure-state-wasm.test.mjs`

It does not run canonical timings or Docker measurements. Those measurements
remain non-gating under ADR-0072.

The shared editor contract lane runs `npm run check:editor-contracts` once. It
owns the shared TypeScript packages and the `criv-wasm` tests. The Obsidian and
VS Code lanes own only their host checks, target-specific Wasm packaging, and
host builds. A host test or build must not call the shared contract command.

### Policy enforcement

Repository policy must not use a custom scanner or a test that only checks for
files, text, commands, or configuration entries. Tests must exercise existing
behavior.

An ADR can include an ast-grep policy only when the decision has a structural
form in supported source code. The decisions in this ADR apply to TOML, JSON,
YAML, Markdown, shell automation, and hosted job results. They do not have an
honest structural-code form, so this ADR does not add an ast-grep policy.

The selected tools enforce their own contracts. Cargo reads `rust-version` and
the lock file. Mise reads its tool and lock files. npm runs the package builds
and tests. GitHub Actions runs the site, performance-tooling, editor, and host
lanes. criv reads the source catalog and validates the vault. These gates test
the operation of the selected tools and products. They do not compare the
repository with a duplicated policy snapshot.

## Consequences

- A new maintained crate, package, extension, fixture, script, or skill enters
  the source index through its parent root.
- Site-only pull requests receive a required CI result and a real site build.
- Performance tooling has normal correctness tests without making machine
  timing a correctness gate.
- Release and development builds use one documented compiler contract.
- Shared editor tests run once under a named owner. Host jobs still prove their
  generated Wasm target and host behavior.
- A deliberate policy change requires a successor ADR. The repository does not
  keep a second policy snapshot for comparison.

## Alternatives Considered

### Keep a smaller curated source catalog

Rejected. A new child directory could remain outside the self-vault until a
maintainer noticed it.

### Declare a lower MSRV than the repository toolchain

Rejected. The repository had no independent compatibility need or hosted lane
for a second compiler contract.

### Use each CI lane as a branch protection rule

Rejected. Renaming or splitting an internal lane would require a branch
protection change. One aggregate gives the external contract a stable name.

### Gate canonical performance timings

Rejected. Shared hosted runners do not provide stable timing evidence, and
ADR-0072 keeps that observation outside correctness gates.

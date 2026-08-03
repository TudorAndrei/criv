---
id: ADR-0060
kind: decision
title: Parallel Hosted Validation And Lean Local Hooks
status: accepted
date: 2026-08-03
supersedes:
  - ADR-0022
  - ADR-0037
  - ADR-0055
governs:
  - .github/workflows/ci.yml
  - hk.pkl
  - mise.toml
  - .obsidian/plugins/criv/package.json
  - extensions/vscode-criv/package.json
  - extensions/vscode-criv/test/run-integration.mjs
---

# Parallel Hosted Validation And Lean Local Hooks

## Context

[[0022-hosted-ci-entry-point|ADR-0022]] made one hosted job invoke the same
`mise run check` entry point as local validation. That prevented command drift,
but the repository has since added independent Rust, Wasm, Obsidian, and VS Code
toolchains. Installing and executing all four surfaces in one job serializes
work that can safely overlap and makes editor-host tests part of every complete
local check.

[[0037-vscode-json-diagnostics-in-hooks|ADR-0037]] also launches a VS Code test
host during pre-commit, while [[0055-dependency-auditing-in-hk-checks|ADR-0055]]
places both hosted npm audits inside the local hk check. These checks remain
valuable, but their latency and network or desktop-host dependencies belong at
the hosted integration boundary rather than in automatic local hooks.

The current workflow already reports Windows separately without gating merges.
Issue #30 owns the later decision to make that job required. The local Hawk
definition from [[0043-hawk-visibility-analysis|ADR-0043]] remains the correct
Rust visibility gate, but omitting it from pre-push allowed a deterministic CI
failure after every configured push check had passed.

## Decision

Hosted pull-request and `main` validation uses four required Linux lanes that
run concurrently where GitHub runner availability permits:

- **core** runs repository-owned Rust formatting, Clippy, workspace tests,
  Hawk, workflow validation, the monitor-only Rust advisory audit, criv vault
  validation, and CI-stage enforcement through `mise run check`;
- **Wasm** builds `crates/criv-wasm` with the pinned `wasm-pack` toolchain;
- **Obsidian** installs its locked npm graph and runs the blocking npm audit,
  formatting, lint, unit tests, TypeScript check, and production plugin bundle;
- **VS Code** installs its locked npm graph and runs the blocking npm audit,
  formatting, lint, unit tests, and extension-host integration diagnostics.

A final job named `Repository checks` depends on all four required lanes, runs
even after a dependency failure, and succeeds only when every required result
is `success`. That stable aggregate is the branch-protection surface. The
Windows build-and-test job remains visible, carries `continue-on-error: true`,
and is deliberately excluded from the aggregate until issue #30 lands.

The core lane invokes `cargo run --quiet -- check --format github` directly
before `mise run check`. The annotation step may fail without pre-empting the
authoritative core gate so raw GitHub workflow commands reach the runner rather
than being prefixed by hk. This decision does not add SARIF, new workflow
permissions, or an enforcement output protocol.

Local automatic validation is narrower. Pre-commit keeps formatting, workflow
security, criv check, and commit enforcement. Pre-push keeps Clippy, workspace
tests, push enforcement, and adds the existing Hawk step. `mise run check`
remains the complete local **core** gate. Obsidian and VS Code checks are not
automatic hk steps; contributors run their package scripts explicitly when
working on a companion, while hosted CI always runs both complete lanes.

[[0049-checks-defined-in-hk-not-mise|ADR-0049]] still governs commands that hk
runs. Companion commands no longer owned by hk live in their package scripts
and are composed by the hosted workflow. The Obsidian production build exposes
independent Wasm and plugin subcommands while retaining the existing combined
`build` behavior for release and manual callers.

VS Code integration tests use a short temporary profile root outside the
checkout for both user data and installed extensions. The runner removes that
root after success or failure. The development extension and workspace paths
remain the real checkout so the test still exercises the shipped integration.

Workflow jobs retain minimal permissions, pinned third-party action commits,
non-persistent checkout credentials, locked npm installation, and the
mise-managed tool versions selected by earlier decisions.

## Consequences

The authoritative hosted result no longer means that one process ran the same
command list as a local machine. It means that one stable aggregate proved all
four independently owned validation surfaces. This adds repeated checkout and
tool-setup cost but reduces critical-path duration and prevents an editor-host
failure from delaying unrelated lanes.

Local commits and pushes stop launching editor tests, npm advisory requests, and
VS Code hosts. Feedback for companion changes therefore arrives later unless a
contributor runs the documented package commands explicitly. In exchange,
automatic hooks remain focused on repository-core correctness and pre-push now
catches Hawk findings before CI.

The Rust audit remains monitor-only because its local advisory database is not
reproducibly refreshed. Obsidian and VS Code npm audits remain blocking in their
hosted lanes. Moving those audits out of hk changes their orchestration, not
their severity policy.

The short VS Code profile fixes path-dependent macOS startup failures during
hosted or manual integration runs. Cleanup is best-effort under normal process
completion; a hard runner termination can still leave a temporary directory for
the operating system to reclaim.

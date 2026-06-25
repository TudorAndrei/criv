---
id: tooling
kind: doc
title: Tooling and Git Hooks
targets:
  symbols:
    - mise.toml
    - hk.pkl
    - scripts/measure-performance.sh
---

# Tooling and Git Hooks

The repository uses [[mise.toml]] to install the project hook runner and
[[hk.pkl]] to define the hook behavior. The decision record is [[0013-mise-managed-hk-hook-toolchain|ADR-0013]].

Run the initial setup with:

```sh
mise install
```

The mise postinstall hook runs `hk install --mise`, so Git hook execution goes
through `mise x` and uses the pinned tool versions, including hk and
actionlint. GitHub Actions security analysis is pinned here too: zizmor checks
workflow, composite action, and Dependabot definitions for risky CI/CD patterns.
Release tooling is also pinned here: Cocogitto calculates the next SemVer
version from conventional commits, and cargo-release updates Cargo workspace
versions. `HK_PKL_BACKEND=pklr` keeps hk self-contained by avoiding a separate
`pkl` CLI requirement.

Workflow YAML under `.github/workflows/` is checked with actionlint in
`pre-commit` and the full `check` hook. zizmor runs in offline mode in the same
hooks so local validation does not require a GitHub token or network access.
This follow-up hook decision is [[0018-offline-zizmor-actions-security-check|ADR-0018]].

Manual task entry points are:

```sh
mise run commit-msg -- .git/COMMIT_EDITMSG
mise run pre-commit
mise run pre-push
mise run check
mise run fix
mise run perf
mise run plugin-build
mise run vscode-build
mise run vscode-test
mise run vscode-lint
mise run vscode-format-check
mise run release-plan
mise run release-auto
```

`mise run perf` runs [[scripts/measure-performance.sh]] against the current
vault by default. Pass another vault root to measure larger repositories:

```sh
mise run perf -- /path/to/large/criv-vault
```

The script records cold and warm `watch --once` timings, source-index file
search startup, validation, CI enforcement, and a no-op snapshot diff. Use those
numbers before changing source graph parsing, source indexing, snapshot writing,
or pattern matching behavior.

`mise run plugin-build` builds the Obsidian companion plugin and its Rust WASM
helper from the repository root. It wraps `.obsidian/plugins/criv`'s
`npm run build` script in `mise x rust@1.95.0`, so `wasm-pack` uses the
mise-managed Rust and Cargo toolchain instead of whichever Rust appears first in
the shell environment.

The VS Code-compatible extension lives in `extensions/vscode-criv`. Its local
tasks are exposed through mise and npm:

```sh
npm --prefix extensions/vscode-criv run build
npm --prefix extensions/vscode-criv run test
npm --prefix extensions/vscode-criv run test:integration
npm --prefix extensions/vscode-criv run lint
npm --prefix extensions/vscode-criv run format:check
npm --prefix extensions/vscode-criv run package
```

The extension renders `.c4` previews locally: Mermaid C4 artifacts use Mermaid
11, DOT Code artifacts use `@viz-js/viz`, and the preview webview uses packaged
extension resources rather than CDN scripts. Run `criv watch --once` after
changing extension source so generated architecture state and
`docs/architecture/04-code.c4` stay current.

`npm --prefix extensions/vscode-criv run package` and `mise run
vscode-package` build a local `vscode-criv.vsix` without publishing. The
extension metadata is kept compatible with both VS Code Marketplace and Open
VSX publication, but MVP verification does not require a marketplace token or
an Open VSX publish step. Install the VSIX explicitly with an editor CLI such
as `code --install-extension` or `cursor --install-extension` when doing
desktop smoke tests.

Use `hk validate` after editing `hk.pkl`. Prefer hk built-ins when one exists;
the commit message hook uses `Builtins.check_conventional_commit` instead of a
shell wrapper around `hk util check-conventional-commit`.

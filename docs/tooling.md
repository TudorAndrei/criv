---
id: tooling
kind: doc
title: Tooling and Git Hooks
targets:
  symbols:
    - criv.toml
    - hawk.toml
    - mise.toml
    - hk.pkl
    - scripts/measure-performance.sh
---

# Tooling and Git Hooks

The repository uses `mise.toml` to install the project hook runner and
`hk.pkl` to define the hook behavior. `criv.toml` also declares project-level
tooling files, including Hawk's `hawk.toml`, as source roots. The decision
record is [[0013-mise-managed-hk-hook-toolchain|ADR-0013]].

Run the initial setup with:

```sh
mise install
```

The mise postinstall hook runs `hk install --mise`, so Git hook execution goes
through `mise x` and uses the pinned tool versions, including hk, Rust 1.97.1,
and actionlint. GitHub Actions security analysis is pinned here too: zizmor
checks workflow, composite action, and Dependabot definitions for risky CI/CD
patterns. Hawk uses the same Rust toolchain to check whether public Rust APIs
are required by the shipped `criv` binary. Release tooling is also pinned here:
Cocogitto calculates the next SemVer version from conventional commits, and
cargo-release updates Cargo workspace versions. `HK_PKL_BACKEND=pklr` keeps hk
self-contained by avoiding a separate `pkl` CLI requirement.

Workflow YAML under `.github/workflows/` is checked with actionlint in
`pre-commit` and the full `check` hook. zizmor runs in offline mode in the same
hooks so local validation does not require a GitHub token or network access.
This follow-up hook decision is [[0018-offline-zizmor-actions-security-check|ADR-0018]].

## Running criv from a hook runner

criv does not install Git hooks and does not set `core.hooksPath`, per
[[0054-criv-does-not-install-git-hooks|ADR-0054]]. Setting that config replaces
the hook directory wholesale, which silently disables whichever runner the
repository already uses. Wire criv into your own runner instead.

The commands are stable:

| Stage | Commands |
|-------|----------|
| commit | `criv watch --once`, `criv check`, `criv enforce --stage commit` |
| push | `criv enforce --stage push` |

`watch --once` refreshes `.criv/state.json` so the `check` that follows validates
current state. Run them in that order.

With hk, as this repository does in `hk.pkl`:

```pkl
local criv_check = new Step {
  glob = List("**/*.md", "criv.toml")
  check_first = true
  check = "criv check"
  fix = "criv check --fix"
}

local criv_enforce_commit = new Step {
  check = "criv enforce --stage commit"
}

hooks {
  ["pre-commit"] {
    fix = true
    steps {
      ["criv-check"] = criv_check
      ["criv-enforce"] = criv_enforce_commit
    }
  }
}
```

With [lefthook](https://lefthook.dev), in `lefthook.yml`:

```yaml
pre-commit:
  parallel: false
  commands:
    criv-watch:
      run: criv watch --once
    criv-check:
      run: criv check
    criv-enforce:
      run: criv enforce --stage commit

pre-push:
  commands:
    criv-enforce:
      run: criv enforce --stage push
```

Keep `parallel: false` for the commit stage: `check` reads the state `watch`
writes, so they must not overlap.

Manual task entry points are:

```sh
mise run commit-msg -- .git/COMMIT_EDITMSG
mise run pre-commit
mise run pre-push
mise run check
mise run fix
mise run perf
mise run vscode-build
mise run vscode-package
mise run release-plan
mise run release-auto
```

Checks and tests are not mise tasks. Every step hk runs is defined in `hk.pkl`
with its command inline, per
[[0049-checks-defined-in-hk-not-mise|ADR-0049]]. To run one on its own:

```sh
hk check --all --plan                        # list every step
hk check --all --step hawk
hk check --all --step plugin-test
hk check --all --step vscode-json-diagnostics
```

`mise run perf` runs `scripts/measure-performance.sh` against the current
vault by default. Pass another vault root to measure larger repositories:

```sh
mise run perf -- /path/to/large/criv-vault
```

The script records cold and warm `watch --once` timings, source-index file
search startup, validation, CI enforcement, and a no-op snapshot diff. Use those
numbers before changing source graph parsing, source indexing, snapshot writing,
or pattern matching behavior.

The `plugin-build` hk step runs `npm --prefix .obsidian/plugins/criv run build`,
which builds the Obsidian companion plugin and its Rust WASM helper. That npm
script invokes `wasm-pack` through `mise exec cargo:wasm-pack@0.15.0
rust@1.97.1`, so the pinned Rust and Cargo toolchain is used rather than whichever
Rust appears first in the shell environment. The pinning lives in the plugin's
own `package.json`, so it holds no matter who invokes the script.

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

The `vscode-json-diagnostics` hk step launches the VS Code extension integration
test host and fails on JSON diagnostics for extension manifest and language
configuration files. This keeps hook validation aligned with the warnings shown
in VS Code's Problems panel.

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

The `hawk` hk step runs Hawk with warnings denied against the shipped `criv` CLI.
The `criv-wasm` crate is excluded because its exported functions are consumed by
the separately built WASM artifacts, outside the CLI binary's Cargo graph. The
full `check` hook runs Hawk whenever Rust sources, Cargo metadata, or its
configuration change. Hawk uses `target/hawk` for its instrumented Cargo
artifacts so its compiler work stays isolated from the other parallel checks.
This policy is captured in
[[0043-hawk-visibility-analysis|ADR-0043]].

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
| commit | `criv watch --once`, `criv check --changed`, `criv enforce --stage commit` |
| push | `criv enforce --stage push` |

`watch --once` refreshes `.criv/state.json` before the changed check. The
changed check is a staged, read-only partial fast path; plain `criv check`
remains the full manual and hosted authority per
[[0067-staged-changes-are-a-partial-check-scope|ADR-0067]]. Run commit commands
in the listed order.

With hk, as this repository does in `hk.pkl`:

```pkl
local criv_check_changed = new Step {
  glob = List("**/*.md", "criv.toml")
  check_first = true
  check = "criv check --changed"
  fix = "criv check --fix"
}

local criv_enforce_commit = new Step {
  check = "criv enforce --stage commit"
}

hooks {
  ["pre-commit"] {
    fix = true
    steps {
      ["criv-check"] = criv_check_changed
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
      run: criv check --changed
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
mise run fix
mise run perf
mise run vscode-build
mise run vscode-package
mise run release-plan
mise run release-auto
```

Pre-commit and pre-push are the automatic local validation boundary. Agents and
contributors do not need to replay them with an aggregate command after each
commit. The former local aggregate task has been removed.

Hosted CI invokes the complete hk `check` profile directly. Every step hk runs
is defined in `hk.pkl` with its command inline, per
[[0049-checks-defined-in-hk-not-mise|ADR-0049]]. To inspect or debug one core
step explicitly:

```sh
hk check --all --plan                        # list every step
hk check --all --step hawk
```

Pre-push runs Clippy, workspace tests, Hawk, and criv push enforcement. Hawk is
kept in both pre-push and the hosted CI profile so unnecessary public Rust APIs
fail before hosted CI during normal local development.

`mise run perf` generates isolated vaults from the checked-in `barrs-small`
and `criv-medium` manifests, then measures an explicit release binary. Build
that binary first and identify its profile at the command line:

```sh
cargo build --release
mise run perf -- --binary target/release/criv --profile release
```

The script records repeated samples for cold, warm, and changed `watch --once`,
source-index file search startup, validation, CI enforcement, docs-only
`next-adr-id`, `orphan-docs`, and `nodes --kind doc` queries, and a no-op
snapshot diff. Five samples are collected by default; use `--samples` to
change the count or repeat `--case` to select cases:

```sh
mise run perf -- --binary target/release/criv --profile release --samples 9
mise run perf -- --binary target/release/criv --profile release --case check
```

Every recorded sample receives a fresh generated vault. Warm cases perform an
untimed state-building run inside that vault before the recorded command, and
the changed case mutates the manifest's declared number of supported source
files without changing their sizes. A unique result directory preserves the
run identity, exact manifest copies, raw JSONL samples, stdout/stderr, and a
JSON summary with min/median/max and median absolute deviation. Failed samples
remain in the raw evidence and make the harness fail.

`mise run perf-container` runs one ignored smoke case through Testcontainers.
It builds criv and the harness inside a digest-pinned Rust Linux image. Docker
is only an optional execution environment: the same checked-in manifests and
generator define the vaults, results have their own container machine identity,
and this task is not part of hooks or hosted CI. A Docker-API-compatible runtime
is required. Large-workload measurement remains deferred.

Obsidian and VS Code validation are hosted-CI suites rather than automatic hk
steps. Contributors changing a companion should run its package scripts
directly. Run `npm ci` from the repository root first. The Obsidian companion
commands are:

```sh
npm audit --workspace criv-obsidian-plugin --audit-level=high
npm --prefix .obsidian/plugins/criv run format:check
npm --prefix .obsidian/plugins/criv run lint
npm --prefix .obsidian/plugins/criv test
npm --prefix .obsidian/plugins/criv run build:wasm
npm --prefix .obsidian/plugins/criv run build:plugin
npm --prefix .obsidian/plugins/criv run build
```

The combined build preserves the release and manual entry point by running the
Wasm build followed by the plugin build. The Wasm command invokes `wasm-pack`
through `mise exec cargo:wasm-pack@0.15.0`, so the repository's pinned tools are
used rather than whichever Rust or `wasm-pack` appears first in the shell.
It writes `criv_wasm.js`, `criv_wasm_bg.js`, and `criv_wasm_bg.wasm` under
`.obsidian/plugins/criv/pkg/`. The plugin bundle keeps that runtime import
external, so a distributable plugin must carry the generated `pkg/` directory
beside `main.js`.

If Obsidian reports that the criv Wasm runtime is unavailable, run the combined
build, confirm those three runtime files exist, and reload the plugin. A state
schema or parse error is separate: regenerate `.criv/state.json` with `criv
watch --once` instead of rebuilding Wasm.

The VS Code-compatible extension lives in `extensions/vscode-criv`. Its local
tasks are exposed through mise and npm:

```sh
npm audit --workspace vscode-criv --audit-level=high
npm --prefix extensions/vscode-criv run build
npm --prefix extensions/vscode-criv run test
npm --prefix extensions/vscode-criv run test:integration
npm --prefix extensions/vscode-criv run lint
npm --prefix extensions/vscode-criv run format:check
npm --prefix extensions/vscode-criv run package
```

The integration command launches the VS Code extension test host and fails on
JSON diagnostics for extension manifest and language configuration files. Its
short temporary user-data and extension-installation paths avoid macOS IPC path
limits. Hosted CI runs this command under Xvfb; it is not part of local hooks.

The extension uses a read-only LikeC4 preview as the default `.c4` editor. It
selects the named view owned by the opened file. There is no fallback view: a
file that owns none states so and points at a file that declares a view.
Navigating inside the
diagram opens the file that owns the target view in the same editor group, so
the tab tracks the diagram. Use
**Reopen Editor With → Text Editor** to edit the DSL. The preview webview uses packaged extension
resources and does not use a CDN or a global LikeC4 command. Run `criv watch
--once` after architecture or extension source changes. This keeps architecture
state current. The repository Code model and views are hand-authored under
`docs/architecture/code/`, so watch does not replace them.

`npm --prefix extensions/vscode-criv run package` and `mise run
vscode-package` build a local `vscode-criv.vsix` without publishing. `mise run
vscode-install` builds that VSIX and installs it into the local editor; reload
the window afterwards. The
prepublish hook builds the Node.js Wasm target into
`extensions/vscode-criv/pkg/`; the VSIX includes `criv_wasm.js` and
`criv_wasm_bg.wasm` from that directory. If the extension reports an unavailable
Wasm runtime, rebuild `build:wasm`, recreate the VSIX, confirm those files are
present in the package, and reload the editor window. Regenerate invalid state
with `criv watch --once`; do not treat a state-validation error as a runtime
packaging failure.

The optional viewer is local-only under
[[0087-keep-editor-setup-out-of-init|ADR-0087]]. The project does not
publish it to the VS Code Marketplace or Open VSX. Release archives put the
one viewer package next to the criv executable. Install it with an explicit
editor selection:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Use `--dry-run` to validate the bundled viewer and local editor command without
changing editor state.

Use `hk validate` after editing `hk.pkl`. Prefer hk built-ins when one exists;
the commit message hook uses `Builtins.check_conventional_commit` instead of a
shell wrapper around `hk util check-conventional-commit`.

The `hawk` hk step runs Hawk with warnings denied against the shipped `criv` CLI.
The `criv-wasm` crate is excluded because its exported functions are consumed by
the separately built WASM artifacts, outside the CLI binary's Cargo graph. The
pre-push and full `check` hooks run Hawk whenever Rust sources, Cargo metadata,
or its configuration change. Hawk uses `target/hawk` for its instrumented Cargo
artifacts so its compiler work stays isolated from the other parallel checks.
This policy is captured in [[0043-hawk-visibility-analysis|ADR-0043]] and the
local/hosted validation boundary is
[[0061-hook-owned-local-validation-and-direct-ci-profile|ADR-0061]].

# Plan 016 Editor Extension Install Path Spike

Date: 2026-07-06

## Scope

This spike designs the explicit editor-extension install path promised by the
README. No production code or CLI flags were changed. The proof of concept used
real `code` and `cursor` CLIs with isolated `--user-data-dir` and
`--extensions-dir` paths under `/private/tmp`, so the existing user editor
profiles were not modified.

ADR-0035 remains the design boundary: `criv init` may recommend the extension,
but default initialization must not mutate user-level editor state.

## Current State

- `criv init` supports `--no-skills`, `--no-obsidian`, `--no-vscode`,
  `--no-hooks`, and `--force-hooks`.
- `criv init` writes `.vscode/extensions.json` with extension ID
  `criv.vscode-criv`, but it does not install the extension.
- README says a future explicit install path may shell out to `code` or
  `cursor` with a published extension ID or local `.vsix`.
- `extensions/vscode-criv/package.json` can package a local
  `vscode-criv.vsix`; the extension is not published to a marketplace yet.

## PoC Transcript

Drift check:

```text
$ git diff --stat 6295490..HEAD -- src/init.rs README.md extensions/vscode-criv/package.json
# no output
```

Editor probes:

```text
$ command -v code
/opt/homebrew/bin/code

$ code --version
1.127.0
4fe60c8b1cdac1c4c174f2fb180d0d758272d713
arm64

$ command -v cursor
/opt/homebrew/bin/cursor

$ cursor --version
3.9.16
042b3c1a4c53f2c3808067f519fbfc67b72cad80
arm64
```

Both CLIs emitted macOS Electron/codesign warnings on stderr, but exited 0 for
the version probes.

Package:

```text
$ mise run vscode-package
[vscode-package] $ npm --prefix extensions/vscode-criv run package
...
DONE  Packaged: vscode-criv.vsix (15 files, 1.49 MB)
```

VS Code isolated install:

```text
$ code --user-data-dir /private/tmp/criv-editor-install-poc/code-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/code-ext \
    --install-extension /Users/tudor/cave/criv/extensions/vscode-criv/vscode-criv.vsix
Installing extensions...
Extension 'vscode-criv.vsix' was successfully installed.
```

VS Code reinstall into the same isolated profile:

```text
$ code --user-data-dir /private/tmp/criv-editor-install-poc/code-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/code-ext \
    --install-extension /Users/tudor/cave/criv/extensions/vscode-criv/vscode-criv.vsix
Installing extensions...
Extension 'vscode-criv.vsix' was successfully installed.
```

VS Code list and cleanup:

```text
$ code --user-data-dir /private/tmp/criv-editor-install-poc/code-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/code-ext \
    --list-extensions | rg '^criv\.vscode-criv$'
criv.vscode-criv

$ code --user-data-dir /private/tmp/criv-editor-install-poc/code-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/code-ext \
    --uninstall-extension criv.vscode-criv
Uninstalling criv.vscode-criv...
Extension 'criv.vscode-criv' was successfully uninstalled!

$ code --user-data-dir /private/tmp/criv-editor-install-poc/code-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/code-ext \
    --list-extensions | rg '^criv\.vscode-criv$'
# exit 1, no output
```

VS Code bad VSIX path:

```text
$ code --user-data-dir /private/tmp/criv-editor-install-poc/code-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/code-ext \
    --install-extension /private/tmp/criv-editor-install-poc/missing.vsix
Installing extensions...
Error: ENOENT: no such file or directory, open '/private/tmp/criv-editor-install-poc/missing.vsix'
Failed Installing Extensions: file:///private/tmp/criv-editor-install-poc/missing.vsix
# exit 1
```

Cursor isolated install/list/uninstall:

```text
$ cursor --user-data-dir /private/tmp/criv-editor-install-poc/cursor-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/cursor-ext \
    --install-extension /Users/tudor/cave/criv/extensions/vscode-criv/vscode-criv.vsix
Installing extensions...
Extension 'vscode-criv.vsix' was successfully installed.

$ cursor --user-data-dir /private/tmp/criv-editor-install-poc/cursor-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/cursor-ext \
    --list-extensions | rg '^criv\.vscode-criv$'
criv.vscode-criv

$ cursor --user-data-dir /private/tmp/criv-editor-install-poc/cursor-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/cursor-ext \
    --uninstall-extension criv.vscode-criv
Uninstalling criv.vscode-criv...
Extension 'criv.vscode-criv' was successfully uninstalled!

$ cursor --user-data-dir /private/tmp/criv-editor-install-poc/cursor-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/cursor-ext \
    --list-extensions | rg '^criv\.vscode-criv$'
# exit 1, no output
```

Cursor bad VSIX path:

```text
$ cursor --user-data-dir /private/tmp/criv-editor-install-poc/cursor-user \
    --extensions-dir /private/tmp/criv-editor-install-poc/cursor-ext \
    --install-extension /private/tmp/criv-editor-install-poc/missing.vsix
Installing extensions...
Error: ENOENT: no such file or directory, open '/private/tmp/criv-editor-install-poc/missing.vsix'
Failed Installing Extensions: file:///private/tmp/criv-editor-install-poc/missing.vsix
# exit 1
```

Missing editor CLI:

```text
$ /private/tmp/criv-editor-install-poc/no-such-editor --version
zsh:1: no such file or directory: /private/tmp/criv-editor-install-poc/no-such-editor
# exit 127
```

Cleanup:

```text
$ git check-ignore -v extensions/vscode-criv/vscode-criv.vsix
.gitignore:13:extensions/vscode-criv/*.vsix extensions/vscode-criv/vscode-criv.vsix

$ rm -rf /private/tmp/criv-editor-install-poc
```

The real user profiles already had `criv.vscode-criv` installed before this
spike, so the PoC did not uninstall from those profiles.

## Design Answers

### 1. Command Surface

Recommendation: add a separate command, not an `init` flag:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
criv install-editor --editor code --vsix path/to/vscode-criv.vsix
```

`init` is repository scaffolding and is safe to rerun. Installing into an
editor mutates user-level state outside the repository, so it deserves an
explicit verb. `criv init` should keep writing `.vscode/extensions.json` by
default and may print a short hint pointing to `criv install-editor`.

### 2. Artifact Source

Recommendation: make published-ID install the normal path, and support
`--vsix <path>` as the developer/offline override.

The local VSIX works today, but target repositories running `criv init` will not
have `extensions/vscode-criv/vscode-criv.vsix`. Teaching criv to download a
VSIX from GitHub releases would introduce network, version-selection, and
integrity policy questions. Published-ID install is the clean user path once the
extension is available in the relevant marketplace or Open VSX registry:

```sh
code --install-extension criv.vscode-criv
cursor --install-extension criv.vscode-criv
```

Until publication exists, the implemented command should either refuse the
default path with a clear "extension is not published yet" message or remain
behind the `--vsix` override for development builds.

### 3. Editor Detection

Recommendation: require explicit editor selection first, with optional
`--editor auto` later.

Supported values should start with `code` and `cursor`, each mapping to the CLI
binary of the same name. Follow the `run_optional_tool` precedent from
`src/enforce.rs`: probe the selected binary, print a clear skip/failure message
when it is not on `PATH`, and never make `criv init` itself fail because an
editor CLI is absent.

Automatic probing is attractive but ambiguous when both VS Code and Cursor are
installed, as they are on this machine. Explicit selection avoids installing
into the wrong editor.

### 4. Failure UX

Recommendation: use precise, non-panicking errors and preserve editor stderr.

Suggested messages:

- Missing CLI:
  `editor install skipped: code was not found on PATH; install VS Code's shell command or pass --editor cursor`
- Missing VSIX:
  `editor install failed: VSIX path does not exist: /path/to/file.vsix`
- Editor nonzero exit:
  `editor install failed: code --install-extension exited 1`, followed by the
  captured stderr/stdout.
- Already installed:
  treat exit 0 as success. The PoC showed both VS Code and Cursor report
  successful install when the VSIX is installed again into the same isolated
  profile.

The command should return nonzero when the user explicitly requested an install
and it could not be completed. That is different from `criv init`, which should
keep editor installation opt-in and non-default.

### 5. Testability

Recommendation: test with fake editor CLIs on `PATH`; do not require real VS
Code/Cursor in CI.

The init tests already use temp directories and subprocess-facing fixtures.
The follow-up can add a temp `bin/code` script that records argv and exits with
controlled status codes. Tests should cover:

- published-ID install argv
- `--vsix` install argv
- missing CLI
- bad VSIX path rejected before spawning
- nonzero editor exit preserves output
- explicit editor selection does not auto-probe other editors

## Follow-Up Plan Outline

1. Add a new `install-editor` subcommand to CLI dispatch.
2. Add an `Editor` value enum with at least `code` and `cursor`.
3. Implement a small editor-install module:
   - resolve CLI binary by editor value
   - choose artifact: published ID by default, `--vsix` override when provided
   - validate local VSIX path before spawning
   - run `<editor> --install-extension <artifact>`
   - capture and report stdout/stderr on failure
4. Gate the default published-ID path on the publication decision. Before
   publication, either ship only `--vsix` or return a targeted "not published"
   error for ID installs.
5. Add tests with fake editor CLIs in temp dirs.
6. Update README lines that currently promise a future install path, replacing
   them with the canonical command examples.
7. Update ADR-0035 context if the project decides marketplace/Open VSX
   publication is now part of the extension distribution strategy.

Publication is a separate human-owned decision because it requires publisher
account ownership, token handling, registry choice, and release process changes.

## Verification

- `mise run vscode-package` succeeded and produced an ignored
  `extensions/vscode-criv/vscode-criv.vsix`.
- Real `code` and `cursor` CLIs installed, listed, and uninstalled the VSIX in
  isolated extension directories.
- Missing CLI and missing VSIX failure cases were recorded.
- The isolated PoC directory was removed.

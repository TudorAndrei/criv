---
id: ADR-0086
kind: decision
title: Bundle One Editor Viewer With criv
status: accepted
date: 2026-08-08
supersedes:
  - ADR-0085
governs:
  - src/install_editor.rs
  - src/lib.rs
  - .github/workflows/release.yml
---

# Bundle One Editor Viewer With criv

## Context

[[0085-local-only-optional-editor-viewer|ADR-0085]] keeps the optional editor
viewer out of public extension registries. It added `criv install-editor`, but
it requires the user to supply a VSIX path. This exposes an internal package
location and makes the user find or build an artifact that the criv release
already owns.

criv has one editor viewer. The user must select the target editor, but the
user must not select the viewer package.

## Decision

Use this command surface:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Remove the `--vsix` option. Keep `--editor` required and accept only `code` or
`cursor`. Keep `--dry-run` for validation without an editor change.

Build one `vscode-criv.vsix` for each criv release. Put that package next to
the `criv` executable in every release archive. `install-editor` resolves only
that fixed sibling file and runs `<editor> --install-extension <vsix>`. It
does not accept a package path, search the working directory, download a
package, detect an editor, or install into more than one editor.

Keep the viewer local-only. Do not publish it to the VS Code Marketplace or
Open VSX. `criv init` can recommend its stable extension ID, but it must not
install the viewer.

## Consequences

The normal command needs only the editor selection. A release archive is a
complete criv installation because it contains both the CLI and its optional
viewer package.

An installation that copies only the executable cannot use `install-editor`.
The command reports the missing sibling package and tells the user to install
criv from a release archive. Extension development can still build and install
a local VSIX with the repository tasks.

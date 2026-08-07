---
id: ADR-0085
kind: decision
title: Local-only Optional Editor Viewer
status: accepted
date: 2026-08-07
governs:
  - src/install_editor.rs
  - src/lib.rs
---

# Local-only Optional Editor Viewer

## Context

[[0035-vscode-compatible-companion-extension|ADR-0035]] created a packaged
VS Code-compatible companion. It kept local VSIX installation available before
any registry release and allowed later Marketplace or Open VSX publication.

criv is an agent CLI first. The editor package is an optional viewer for local
State and LikeC4 output. A public extension listing would present the viewer as
a separate product surface. Registry publication would also add publisher
accounts, credentials, release jobs, registry differences, and remote artifact
selection to a local tool.

## Decision

Keep the optional editor viewer locally installable as a VSIX. Do not publish
it to the VS Code Marketplace or Open VSX. Do not add registry installation,
network download, automatic editor detection, or automatic installation.

Add an explicit command:

```sh
criv install-editor --editor code --vsix path/to/vscode-criv.vsix
criv install-editor --editor cursor --vsix path/to/vscode-criv.vsix
```

Both `--editor` and `--vsix` are required. The command accepts only `code` and
`cursor`, checks that the selected CLI is on `PATH`, and checks that the local
artifact is a regular `.vsix` file before it starts the editor. It runs only
`<editor> --install-extension <vsix>`. It preserves editor output when the
editor fails and treats an editor exit status of zero as success, including a
repeat installation.

Provide `--dry-run` to validate the inputs and show the selected local command
without changing editor state. `criv init` remains repository-scoped. It may
recommend the stable extension ID, but it never installs the viewer.

## Consequences

The CLI remains the primary product interface. Users who want the viewer must
first obtain or build a local VSIX and then select the editor explicitly.

There is no one-command install from a public registry. Release automation does
not need publisher accounts or registry credentials. A future public listing
would reverse this product decision and require a new ADR.

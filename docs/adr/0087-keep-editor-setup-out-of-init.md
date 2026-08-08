---
id: ADR-0087
kind: decision
title: Keep Editor Setup Out of init
status: accepted
date: 2026-08-08
supersedes:
  - ADR-0009
  - ADR-0025
  - ADR-0086
governs:
  - src/init.rs
  - src/init/templates.rs
  - src/install_editor.rs
  - .github/workflows/release.yml
  - README.md
---

# Keep Editor Setup Out of init

## Context

[[0009-obsidian-plugin-as-state-consumer|ADR-0009]] and
[[0025-init-ships-plugin-source-not-generated-bundle|ADR-0025]] made
`criv init` create an Obsidian companion scaffold. Later,
[[0086-bundle-one-editor-viewer-with-criv|ADR-0086]] kept the VS Code-compatible
viewer local-only, but allowed `criv init` to recommend it.

These editor actions make initialization look like editor setup. criv is an
agent CLI first. A vault must not get editor files unless a user makes a
separate editor-level choice.

## Decision

Keep `criv init` limited to the criv vault, local generated state, and agent
skills. It must not create or update `.obsidian` or `.vscode` files. Remove the
`--no-obsidian` and `--no-vscode` options because there are no editor actions to
skip.

Keep the editor viewer optional and local-only. Do not publish it to the VS Code
Marketplace or Open VSX. Build one `vscode-criv.vsix` for each criv release and
put it next to the `criv` executable in every release archive.

Use only these explicit installation commands:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Keep `--editor` required and accept only `code` or `cursor`. Keep `--dry-run`.
The command resolves only the fixed sibling `vscode-criv.vsix` file and runs
the selected editor's local extension installation command. It must not accept
a package path, find an editor automatically, download a package, or install
into more than one editor.

The Obsidian companion remains a maintained source package in the criv
repository, but `criv init` does not copy it into another repository. Its
tests and build checks remain part of criv development. The repository tracks
its authored source, not its generated `main.js` bundle. All editor companions
remain consumers of `.criv/state.json`; the CLI stays authoritative for graph
generation, validation, and enforcement.

## Consequences

`criv init` has no editor side effects and no editor-specific options. A user
can initialize criv without selecting or using an editor.

VS Code and Cursor users install the optional viewer with one separate command.
An installation that copies only the executable cannot use `install-editor`.
The command reports the missing sibling package and directs the user to a criv
release archive.

Existing vaults keep editor files that an older `criv init` created. criv does
not remove user files during a later initialization.

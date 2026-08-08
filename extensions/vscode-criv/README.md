# criv

VS Code-compatible companion extension for criv vaults. The extension is built
for VS Code API-compatible desktop editors, including VS Code and Cursor.

## Features

- Loads `.criv/state.json` for the current workspace and shows source,
  pattern, and `.c4` artifact state.
- Provides source selector links, hovers, completions, and lightweight
  diagnostics for Markdown and `.c4` files.
- Runs trusted workspace commands for `criv watch --once`, `criv check
  --format json`, and selected read-only `criv query` flows.
- Renders the validated LikeC4 workspace from criv state. The preview supports
  named views, drill-down navigation, navigation history, pan, zoom, search,
  source links, and SVG export. It is the default read-only editor for `.c4`
  files and selects the view owned by the opened file.

LikeC4 owns the architecture language and its validation. The Rust CLI calls
the pinned local LikeC4 package and publishes its layout model in criv state.
The extension consumes that model and uses the packaged WASM helper for other
editor-local state lookups.

## Compatibility

The extension targets stable VS Code APIs with `engines.vscode` set to
`^1.85.0`. It avoids proposed APIs and host-specific commands so the packaged
VSIX can be installed in VS Code-compatible desktop editors such as Cursor.

The viewer is local-only. It is not published to the VS Code Marketplace or
Open VSX. A criv release archive includes the viewer package next to the CLI.
Install it into one selected editor:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Direct installation is an explicit editor-level action. `criv init` should
recommend the extension for a workspace, but should not install it by default.

Use **Reopen Editor With → Text Editor** when an agent or maintainer must edit
the LikeC4 DSL. The `criv: Preview C4 Artifact` command can open a second
preview beside that text editor.

The workspace can recommend the official `likec4.likec4-vscode` extension for
LikeC4 syntax highlighting, completion, formatting, and language-server
features. It does not register a competing custom editor. Criv remains the
default validated preview and accepts both the official `likec4` language ID
and its standalone `criv-c4` language ID.

## Development

From the repository root:

```sh
npm --prefix extensions/vscode-criv install
npm --prefix extensions/vscode-criv run build
npm --prefix extensions/vscode-criv run test
npm --prefix extensions/vscode-criv run test:integration
npm --prefix extensions/vscode-criv run package
```

The package step builds the TypeScript bundle, rebuilds the WASM runtime into
`pkg/`, and writes `vscode-criv.vsix` without publishing it.

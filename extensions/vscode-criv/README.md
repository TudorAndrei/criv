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
  named views, pan, zoom, search, source links, and SVG export. It opens beside
  `.c4` editors by default and is also available from the editor title button.

LikeC4 owns the architecture language and its validation. The Rust CLI calls
the pinned local LikeC4 package and publishes its layout model in criv state.
The extension consumes that model and uses the packaged WASM helper for other
editor-local state lookups.

## Compatibility

The extension targets stable VS Code APIs with `engines.vscode` set to
`^1.85.0`. It avoids proposed APIs and host-specific commands so the packaged
VSIX can be installed in VS Code-compatible desktop editors such as Cursor.

Install a local VSIX with an editor CLI when available:

```sh
code --install-extension extensions/vscode-criv/vscode-criv.vsix
cursor --install-extension extensions/vscode-criv/vscode-criv.vsix
```

Direct installation is an explicit editor-level action. `criv init` should
recommend the extension for a workspace, but should not install it by default.

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

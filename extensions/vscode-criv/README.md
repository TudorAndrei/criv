# criv

VS Code-compatible companion extension for criv vaults. The extension is built
for VS Code API-compatible desktop editors, including VS Code and Cursor.

## Features

- Loads `.criv/state.json` for the current workspace and shows source,
  documentation asset, and policy pattern state.
- Opens verified documentation images and PDFs from the active State asset
  inventory with the native editor.
- Provides source selector links, hovers, completions, and lightweight
  diagnostics for Markdown files.
- Runs trusted workspace commands for `criv watch --once`, `criv check
  --format json`, and selected read-only `criv query` flows.

The extension consumes the packaged Wasm runtime for State validation, source
lookup, and selector suggestions. It does not compile or render architecture
models.

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

Direct installation is an explicit editor-level action. `criv init` does not
recommend or install the extension.

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

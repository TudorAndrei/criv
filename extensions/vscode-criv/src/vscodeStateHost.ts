import * as vscode from "vscode";

import type { WorkspaceStateHost } from "./stateStore";

export function createVscodeStateHost(): WorkspaceStateHost {
  return {
    async findWorkspaceRoot() {
      for (const folder of vscode.workspace.workspaceFolders ?? []) {
        const configUri = vscode.Uri.joinPath(folder.uri, "criv.toml");
        try {
          await vscode.workspace.fs.stat(configUri);
          return folder.uri;
        } catch {
          // Continue looking for a criv workspace in multi-root windows.
        }
      }
      return undefined;
    },

    stateFile(root) {
      return vscode.Uri.joinPath(root, ".criv", "state.json");
    },

    async readState(stateUri) {
      const bytes = await vscode.workspace.fs.readFile(stateUri);
      return Buffer.from(bytes).toString("utf8");
    },

    watchState(root, refresh) {
      const watcher = vscode.workspace.createFileSystemWatcher(
        new vscode.RelativePattern(root, ".criv/state.json"),
      );
      watcher.onDidCreate(refresh);
      watcher.onDidChange(refresh);
      watcher.onDidDelete(refresh);
      return watcher;
    },
  };
}

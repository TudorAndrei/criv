import * as vscode from "vscode";
import {
  CRIV_STATE_PATH,
  CrivStateMissingError,
  CrivStateStream,
  readCrivState,
  type CrivStateHost,
  type CrivStateStatus,
} from "@criv/editor-state";
import { createPackagedCrivStateModule } from "@criv/editor-state/packaged";

export interface WorkspaceStateSource {
  root: vscode.Uri;
  stateUri: vscode.Uri;
}

export type WorkspaceState = CrivStateStream<WorkspaceStateSource>;
export type WorkspaceStateStatus = CrivStateStatus<WorkspaceStateSource>;

/** The workspace State stream, refreshed when the State file changes. */
export function createWorkspaceState(): WorkspaceState {
  const state: WorkspaceState = new CrivStateStream(
    new WorkspaceStateHost(() => void state.refresh()),
    createPackagedCrivStateModule(
      "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload the editor.",
    ),
  );
  return state;
}

class WorkspaceStateHost implements CrivStateHost<WorkspaceStateSource> {
  private watcher: { root: string; disposable: vscode.Disposable } | undefined;

  constructor(private readonly onChange: () => void) {}

  async locate(): Promise<WorkspaceStateSource> {
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      if (await exists(vscode.Uri.joinPath(folder.uri, "criv.toml"))) {
        this.watch(folder.uri);
        return { root: folder.uri, stateUri: vscode.Uri.joinPath(folder.uri, CRIV_STATE_PATH) };
      }
    }
    this.dispose();
    throw new CrivStateMissingError("Open a workspace containing criv.toml.");
  }

  read(source: WorkspaceStateSource): Promise<string> {
    return readCrivState(CRIV_STATE_PATH, async () =>
      new TextDecoder().decode(await vscode.workspace.fs.readFile(source.stateUri)),
    );
  }

  dispose(): void {
    this.watcher?.disposable.dispose();
    this.watcher = undefined;
  }

  private watch(root: vscode.Uri): void {
    if (this.watcher?.root === root.toString()) {
      return;
    }
    this.dispose();
    const watcher = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(root, CRIV_STATE_PATH),
    );
    const refresh = () => this.onChange();
    watcher.onDidCreate(refresh);
    watcher.onDidChange(refresh);
    watcher.onDidDelete(refresh);
    this.watcher = { root: root.toString(), disposable: watcher };
  }
}

async function exists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

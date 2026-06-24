import * as vscode from "vscode";

import { buildStateSnapshot, parseStateEnvelope, type CrivStateSnapshot } from "./stateModel";
import {
  graphNodes,
  lookupGraphNode,
  sourceEntries,
  suggestSourceSelectors,
  summarizeState,
} from "./wasm";

export type WorkspaceStateStatus =
  | { kind: "loading" }
  | { kind: "ready"; root: vscode.Uri; stateUri: vscode.Uri; snapshot: CrivStateSnapshot }
  | { kind: "missing-workspace"; message: string }
  | { kind: "missing-state"; root: vscode.Uri; stateUri: vscode.Uri; message: string }
  | { kind: "invalid-state"; root: vscode.Uri; stateUri: vscode.Uri; message: string };

export class WorkspaceStateStore implements vscode.Disposable {
  private statusValue: WorkspaceStateStatus = { kind: "loading" };
  private watcher: vscode.FileSystemWatcher | undefined;
  private readonly didChangeStatus = new vscode.EventEmitter<WorkspaceStateStatus>();

  readonly onDidChangeStatus = this.didChangeStatus.event;

  get status(): WorkspaceStateStatus {
    return this.statusValue;
  }

  async refresh(): Promise<WorkspaceStateStatus> {
    this.setStatus({ kind: "loading" });
    const root = await findCrivWorkspaceRoot();
    if (!root) {
      this.disposeWatcher();
      return this.setStatus({
        kind: "missing-workspace",
        message: "Open a workspace containing criv.toml.",
      });
    }

    this.ensureWatcher(root);
    const stateUri = vscode.Uri.joinPath(root, ".criv", "state.json");
    let raw: string;
    try {
      const bytes = await vscode.workspace.fs.readFile(stateUri);
      raw = Buffer.from(bytes).toString("utf8");
    } catch (error) {
      return this.setStatus({
        kind: "missing-state",
        root,
        stateUri,
        message: `Could not read .criv/state.json: ${messageFromError(error)}`,
      });
    }

    const parsed = parseStateEnvelope(raw);
    if (!parsed.ok) {
      return this.setStatus({
        kind: "invalid-state",
        root,
        stateUri,
        message: parsed.error,
      });
    }

    try {
      const [summary, sources, nodes] = await Promise.all([
        summarizeState(raw),
        sourceEntries(raw),
        graphNodes(raw),
      ]);

      return this.setStatus({
        kind: "ready",
        root,
        stateUri,
        snapshot: buildStateSnapshot(raw, parsed.envelope, summary, sources, nodes),
      });
    } catch (error) {
      return this.setStatus({
        kind: "invalid-state",
        root,
        stateUri,
        message: `Could not read criv state projections: ${messageFromError(error)}`,
      });
    }
  }

  async lookupNode(target: string) {
    const status = this.statusValue;
    if (status.kind !== "ready") {
      return undefined;
    }
    return lookupGraphNode(status.snapshot.raw, target);
  }

  async suggestSelectors(query: string, limit: number) {
    const status = this.statusValue;
    if (status.kind !== "ready") {
      return [];
    }
    return suggestSourceSelectors(status.snapshot.raw, query, limit);
  }

  dispose(): void {
    this.disposeWatcher();
    this.didChangeStatus.dispose();
  }

  private setStatus(status: WorkspaceStateStatus): WorkspaceStateStatus {
    this.statusValue = status;
    this.didChangeStatus.fire(status);
    return status;
  }

  private ensureWatcher(root: vscode.Uri): void {
    const watchedRoot = this.watcherRoot();
    if (watchedRoot === root.toString()) {
      return;
    }

    this.disposeWatcher();
    const watcher = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(root, ".criv/state.json"),
    );
    const refresh = () => {
      void this.refresh();
    };
    watcher.onDidCreate(refresh);
    watcher.onDidChange(refresh);
    watcher.onDidDelete(refresh);
    this.watcher = watcher;
    this.watcherRootValue = root.toString();
  }

  private watcherRootValue: string | undefined;

  private watcherRoot(): string | undefined {
    return this.watcherRootValue;
  }

  private disposeWatcher(): void {
    this.watcher?.dispose();
    this.watcher = undefined;
    this.watcherRootValue = undefined;
  }
}

export async function findCrivWorkspaceRoot(): Promise<vscode.Uri | undefined> {
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
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

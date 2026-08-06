import type * as vscode from "vscode";

import { buildStateSnapshot, type CrivStateSnapshot } from "./stateModel";
import {
  CrivWasmLoadError,
  type CrivLoadedState,
  type CrivWasmBridge,
} from "./wasm";

export type WorkspaceStateStatus =
  | { kind: "loading" }
  | { kind: "ready"; root: vscode.Uri; stateUri: vscode.Uri; snapshot: CrivStateSnapshot }
  | { kind: "missing-workspace"; message: string }
  | { kind: "missing-state"; root: vscode.Uri; stateUri: vscode.Uri; message: string }
  | { kind: "wasm-unavailable"; root: vscode.Uri; stateUri: vscode.Uri; message: string }
  | { kind: "invalid-state"; root: vscode.Uri; stateUri: vscode.Uri; message: string };

export interface Disposable {
  dispose(): void;
}

export interface WorkspaceStateHost {
  findWorkspaceRoot(): Promise<vscode.Uri | undefined>;
  stateFile(root: vscode.Uri): vscode.Uri;
  readState(stateUri: vscode.Uri): Promise<string>;
  watchState(root: vscode.Uri, refresh: () => void): Disposable;
}

export class WorkspaceStateStore implements Disposable {
  private statusValue: WorkspaceStateStatus = { kind: "loading" };
  private watcher: Disposable | undefined;
  private watcherRootValue: string | undefined;
  private loaded: CrivLoadedState | undefined;
  private refreshSequence = 0;
  private disposed = false;
  private readonly listeners = new Set<(status: WorkspaceStateStatus) => void>();

  constructor(
    private readonly host: WorkspaceStateHost,
    private readonly bridge: CrivWasmBridge,
  ) {}

  readonly onDidChangeStatus = (
    listener: (status: WorkspaceStateStatus) => void,
  ): Disposable => {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  };

  get status(): WorkspaceStateStatus {
    return this.statusValue;
  }

  async refresh(): Promise<WorkspaceStateStatus> {
    if (this.disposed) {
      return this.statusValue;
    }
    const sequence = ++this.refreshSequence;
    if (!this.loaded) {
      this.setStatus({ kind: "loading" });
    }

    const root = await this.host.findWorkspaceRoot();
    if (!this.isCurrent(sequence)) {
      return this.statusValue;
    }
    if (!root) {
      this.disposeWatcher();
      this.disposeLoaded();
      return this.setStatus({
        kind: "missing-workspace",
        message: "Open a workspace containing criv.toml.",
      });
    }

    this.ensureWatcher(root);
    const stateUri = this.host.stateFile(root);
    let raw: string;
    try {
      raw = await this.host.readState(stateUri);
    } catch (error) {
      if (!this.isCurrent(sequence)) {
        return this.statusValue;
      }
      this.disposeLoaded();
      return this.setStatus({
        kind: "missing-state",
        root,
        stateUri,
        message: `Could not read .criv/state.json: ${messageFromError(error)}`,
      });
    }

    let candidate: CrivLoadedState | undefined;
    try {
      candidate = await this.bridge.loadState(raw);
      if (!this.isCurrent(sequence)) {
        candidate.dispose();
        return this.statusValue;
      }
      const projections = candidate.initialProjections();
      const snapshot = buildStateSnapshot(
        projections.state,
        projections.summary,
        projections.sources,
        projections.nodes,
      );
      const previous = this.loaded;
      this.loaded = candidate;
      candidate = undefined;
      const status = this.setStatus({ kind: "ready", root, stateUri, snapshot });
      previous?.dispose();
      return status;
    } catch (error) {
      candidate?.dispose();
      if (!this.isCurrent(sequence)) {
        return this.statusValue;
      }
      this.disposeLoaded();
      return this.setStatus({
        kind: error instanceof CrivWasmLoadError ? "wasm-unavailable" : "invalid-state",
        root,
        stateUri,
        message:
          error instanceof CrivWasmLoadError
            ? error.message
            : `Could not read criv state projections: ${messageFromError(error)}`,
      });
    }
  }

  lookupNode(target: string) {
    return this.loaded?.lookupNode(target);
  }

  suggestSelectors(query: string, limit: number) {
    return this.loaded?.suggestSelectors(query, limit) ?? [];
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.refreshSequence += 1;
    this.disposeWatcher();
    this.disposeLoaded();
    this.listeners.clear();
  }

  private isCurrent(sequence: number): boolean {
    return !this.disposed && sequence === this.refreshSequence;
  }

  private setStatus(status: WorkspaceStateStatus): WorkspaceStateStatus {
    this.statusValue = status;
    for (const listener of this.listeners) {
      listener(status);
    }
    return status;
  }

  private ensureWatcher(root: vscode.Uri): void {
    if (this.watcherRootValue === root.toString()) {
      return;
    }
    this.disposeWatcher();
    this.watcher = this.host.watchState(root, () => {
      void this.refresh();
    });
    this.watcherRootValue = root.toString();
  }

  private disposeWatcher(): void {
    this.watcher?.dispose();
    this.watcher = undefined;
    this.watcherRootValue = undefined;
  }

  private disposeLoaded(): void {
    this.loaded?.dispose();
    this.loaded = undefined;
  }
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

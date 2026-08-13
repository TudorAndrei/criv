import type * as vscode from "vscode";
import { LoadedRevisionOwner } from "@criv/editor-state";

import { buildStateSnapshot, type CrivStateSnapshot } from "./model";
import { CrivWasmLoadError, type CrivLoadedState, type CrivWasmBridge } from "./wasm";

export type WorkspaceStateStatus =
  | { generation: number; kind: "loading" }
  | {
      generation: number;
      kind: "ready";
      root: vscode.Uri;
      stateUri: vscode.Uri;
      snapshot: CrivStateSnapshot;
    }
  | { generation: number; kind: "missing"; reason: "workspace"; message: string }
  | {
      generation: number;
      kind: "missing";
      reason: "state";
      root: vscode.Uri;
      stateUri: vscode.Uri;
      message: string;
    }
  | {
      generation: number;
      kind: "unavailable";
      root: vscode.Uri;
      stateUri: vscode.Uri;
      message: string;
    }
  | {
      generation: number;
      kind: "invalid";
      root: vscode.Uri;
      stateUri: vscode.Uri;
      message: string;
    };

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
  private statusValue: WorkspaceStateStatus = { generation: 0, kind: "loading" };
  private nextGeneration = 0;
  private watcher: Disposable | undefined;
  private watcherRootValue: string | undefined;
  private readonly revisions = new LoadedRevisionOwner<CrivLoadedState>();
  private disposed = false;
  private readonly listeners = new Set<(status: WorkspaceStateStatus) => void>();

  constructor(
    private readonly host: WorkspaceStateHost,
    private readonly bridge: CrivWasmBridge,
  ) {}

  readonly onDidChangeStatus = (listener: (status: WorkspaceStateStatus) => void): Disposable => {
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
    const generation = ++this.nextGeneration;
    if (!this.revisions.current) {
      this.setStatus({ generation, kind: "loading" });
    }

    let root: vscode.Uri | undefined;
    let stateUri: vscode.Uri | undefined;
    const result = await this.revisions.replace(
      async (attempt) => {
        root = await this.host.findWorkspaceRoot();
        attempt.assertCurrent();
        if (!root) {
          throw new WorkspaceStateHostError("missing-workspace");
        }

        this.ensureWatcher(root);
        stateUri = this.host.stateFile(root);
        let raw: string;
        try {
          raw = await this.host.readState(stateUri);
        } catch (error) {
          throw new WorkspaceStateHostError("missing-state", { cause: error });
        }
        attempt.assertCurrent();
        return this.bridge.loadState(raw);
      },
      (candidate) => {
        const projections = candidate.initialProjections();
        return buildStateSnapshot(projections);
      },
    );

    if (result.kind !== "committed" && result.kind !== "failed") {
      return this.statusValue;
    }
    if (result.kind === "committed") {
      return this.setStatus({
        generation,
        kind: "ready",
        root: root!,
        stateUri: stateUri!,
        snapshot: result.value,
      });
    }

    const error = result.error;
    if (error instanceof WorkspaceStateHostError && error.kind === "missing-workspace") {
      this.disposeWatcher();
      return this.setStatus({
        generation,
        kind: "missing",
        reason: "workspace",
        message: "Open a workspace containing criv.toml.",
      });
    }
    if (error instanceof WorkspaceStateHostError && error.kind === "missing-state") {
      return this.setStatus({
        generation,
        kind: "missing",
        reason: "state",
        root: root!,
        stateUri: stateUri!,
        message: `Could not read .criv/state.json: ${messageFromError(error.cause)}`,
      });
    }
    return this.setStatus({
      generation,
      kind: error instanceof CrivWasmLoadError ? "unavailable" : "invalid",
      root: root!,
      stateUri: stateUri!,
      message:
        error instanceof CrivWasmLoadError
          ? error.message
          : `Could not read criv state projections: ${messageFromError(error)}`,
    });
  }

  lookupSourceTarget(target: string) {
    return this.revisions.current?.lookupSourceTarget(target) ?? { kind: "unresolved" as const };
  }

  suggestSelectors(query: string, limit: number) {
    return this.revisions.current?.suggestSelectors(query, limit) ?? [];
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.disposeWatcher();
    this.revisions.dispose();
    this.listeners.clear();
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
}

class WorkspaceStateHostError extends Error {
  constructor(
    readonly kind: "missing-workspace" | "missing-state",
    options?: ErrorOptions,
  ) {
    super(kind, options);
    this.name = "WorkspaceStateHostError";
  }
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

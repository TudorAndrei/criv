import { Notice } from "obsidian";
import type { App } from "obsidian";
import { LoadedRevisionOwner } from "@criv/editor-state";
import type { CrivState, SourceIndexEntry, SourceResolver } from "./core";
import { safeVaultPath } from "./core";
import type { DisposableSubscription, ObsidianStateStatus, StatePort } from "./ports";
import {
  CrivWasmLoadError,
  type CrivLoadedState,
  type CrivSelectorSuggestion,
  type CrivStateSummary,
} from "./wasm";

interface StateFileToken {
  mtime: number;
  size: number;
}

export class ObsidianStateOwner implements StatePort {
  private state: CrivState | null = null;
  private stateSources: SourceIndexEntry[] = [];
  private stateSourcesByPath = new Map<string, SourceIndexEntry>();
  private stateSummary: CrivStateSummary | null = null;
  private readonly revisions = new LoadedRevisionOwner<CrivLoadedState>();
  private stateToken: StateFileToken | null = null;
  private stateError: string | null = null;
  private statusValue: ObsidianStateStatus = { generation: 0, kind: "loading" };
  private nextGeneration = 0;
  private readonly listeners = new Set<(status: ObsidianStateStatus) => void>();
  private disposed = false;
  private wasmFailureNotified = false;

  constructor(
    private readonly app: App,
    private readonly statePath: () => string,
    private readonly loadWasmRevision: (raw: string) => Promise<CrivLoadedState>,
  ) {}

  async readState(): Promise<CrivStateSummary | null> {
    await this.getState();
    return this.stateSummary;
  }

  async showStatus(): Promise<void> {
    const state = await this.readState();
    if (!state) {
      new Notice(`criv state is missing at ${this.statePath()}`);
      return;
    }
    new Notice(
      `criv ${state.schema}: ${state.node_count} nodes, ${state.edge_count} edges, ${state.source_count} source files`,
    );
  }

  async loadState(observedToken?: StateFileToken | null): Promise<CrivState | null> {
    if (this.disposed) {
      return this.state;
    }
    const generation = ++this.nextGeneration;
    if (!this.revisions.current) {
      this.publish({ generation, kind: "loading" });
    }
    const configuredPath = this.statePath();
    let resolvedPath: string | null = null;
    let token: StateFileToken | null = null;
    const result = await this.revisions.replace(
      async (attempt) => {
        resolvedPath = safeVaultPath(configuredPath);
        if (!resolvedPath) {
          throw new Error(`Invalid criv state path ${configuredPath}.`);
        }
        token =
          observedToken === undefined ? await this.readStateFileToken(resolvedPath) : observedToken;
        attempt.assertCurrent();
        let raw: string;
        try {
          raw = await this.app.vault.adapter.read(resolvedPath);
        } catch (error) {
          throw new ObsidianStateReadError(error);
        }
        attempt.assertCurrent();
        return this.loadWasmRevision(raw);
      },
      (candidate) => candidate.initialProjections(),
    );

    if (result.kind !== "committed" && result.kind !== "failed") {
      return this.state;
    }
    if (result.kind === "committed") {
      this.state = {
        registeredPatterns: result.value.registeredPatterns,
        patternMatches: result.value.patternMatches,
        architecture: result.value.architecture,
      };
      this.stateSources = result.value.sources;
      this.stateSourcesByPath = new Map(
        result.value.sources.map((source) => [source.path, source]),
      );
      this.stateSummary = result.value.summary;
      this.stateToken = token;
      this.stateError = null;
      this.publish({ generation, kind: "ready", state: this.state });
      return this.state;
    }

    this.clearCache();
    this.stateToken = token;
    this.recordWasmFailure(result.error);
    this.stateError =
      resolvedPath === null
        ? `Invalid criv state path ${configuredPath}.`
        : result.error instanceof ObsidianStateReadError
          ? `Could not read ${resolvedPath}: ${errorMessage(result.error.cause)}`
          : result.error instanceof CrivWasmLoadError
            ? result.error.message
            : `Could not read ${resolvedPath}: ${errorMessage(result.error)}`;
    this.publish({
      generation,
      kind:
        result.error instanceof ObsidianStateReadError
          ? "missing"
          : result.error instanceof CrivWasmLoadError
            ? "unavailable"
            : "invalid",
      message: this.stateError,
    });
    return null;
  }

  async observeFile(): Promise<void> {
    if (this.disposed) {
      return;
    }
    const path = safeVaultPath(this.statePath());
    if (!path) {
      return;
    }
    const token = await this.readStateFileToken(path);
    if (sameStateFileToken(token, this.stateToken)) {
      return;
    }
    await this.loadState(token);
  }

  currentStateStatus(): ObsidianStateStatus {
    return this.statusValue;
  }

  onStateStatusChange(listener: (status: ObsidianStateStatus) => void): DisposableSubscription {
    this.listeners.add(listener);
    let disposed = false;
    return {
      dispose: () => {
        if (disposed) {
          return;
        }
        disposed = true;
        this.listeners.delete(listener);
      },
    };
  }

  async getState(): Promise<CrivState | null> {
    return this.state ?? (await this.loadState());
  }

  stateStatus(): string {
    return this.stateError ?? `criv state is unavailable at ${this.statePath()}.`;
  }

  cachedState(): CrivState | null {
    return this.state;
  }

  cachedSourceResolver(): SourceResolver {
    return {
      lookupSourceTarget: (target) =>
        this.revisions.current?.lookupSourceTarget(target) ?? { kind: "unresolved" },
      sourceEntry: (path) => this.stateSourcesByPath.get(path),
    };
  }

  suggestSourceSelectors(query: string, limit: number): CrivSelectorSuggestion[] {
    return this.revisions.current?.suggestSelectors(query, limit) ?? [];
  }

  recordWasmFailure(error: unknown): void {
    if (!(error instanceof CrivWasmLoadError) || this.wasmFailureNotified) {
      return;
    }
    this.wasmFailureNotified = true;
    new Notice(error.message);
  }

  async sourceEntries(): Promise<SourceIndexEntry[]> {
    await this.getState();
    return this.stateSources.slice();
  }

  async patternIds(): Promise<string[]> {
    const state = await this.getState();
    return state?.registeredPatterns ?? [];
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.listeners.clear();
    this.revisions.dispose();
    this.clearCache();
  }

  private clearCache(): void {
    this.state = null;
    this.stateSources = [];
    this.stateSourcesByPath.clear();
    this.stateSummary = null;
    this.stateToken = null;
  }

  private publish(status: ObsidianStateStatus): void {
    if (this.disposed) {
      return;
    }
    this.statusValue = status;
    for (const listener of this.listeners) {
      listener(status);
    }
  }

  private async readStateFileToken(path: string): Promise<StateFileToken | null> {
    try {
      const stat = await this.app.vault.adapter.stat(path);
      return stat ? { mtime: stat.mtime, size: stat.size } : null;
    } catch {
      return null;
    }
  }
}

class ObsidianStateReadError extends Error {
  readonly cause: unknown;

  constructor(cause: unknown) {
    super("The criv State file could not be read.");
    this.name = "ObsidianStateReadError";
    this.cause = cause;
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function sameStateFileToken(left: StateFileToken | null, right: StateFileToken | null): boolean {
  return left?.mtime === right?.mtime && left?.size === right?.size;
}

import { Notice } from "obsidian";
import type { App } from "obsidian";
import {
  CrivStateMissingError,
  CrivStateStream,
  crivStatusMessage,
  readCrivState,
  safeVaultPath,
  type CrivInitialProjections,
  type CrivSelectorSuggestion,
  type CrivSourceEntry,
  type CrivStateHost,
  type CrivStateStatus,
  type Disposable,
} from "@criv/editor-state";
import { createPackagedCrivStateModule } from "@criv/editor-state/packaged";

import type { SourceResolver, StatePort } from "../ports";

interface StateFileToken {
  mtime: number;
  size: number;
}

/** Owns the State file, its loaded revision, and the status stream for the vault. */
export class ObsidianStateOwner implements StatePort {
  private readonly host: ObsidianStateHost;
  private readonly stream: CrivStateStream<string>;
  private sourcesByPath = new Map<string, CrivSourceEntry>();
  private wasmFailureNotified = false;

  constructor(app: App, statePath: () => string) {
    this.host = new ObsidianStateHost(app, statePath);
    this.stream = new CrivStateStream(
      this.host,
      createPackagedCrivStateModule(
        "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload Obsidian.",
      ),
    );
    this.stream.onDidChange((status) => this.observe(status));
  }

  refresh(): Promise<CrivStateStatus<string>> {
    return this.stream.refresh();
  }

  /** Reload State when the State file changed since the last read. */
  async poll(): Promise<void> {
    if (await this.host.changed()) {
      await this.stream.refresh();
    }
  }

  onStatusChange(listener: (status: CrivStateStatus<string>) => void): Disposable {
    return this.stream.onDidChange(listener);
  }

  async showStatus(): Promise<void> {
    const projections = await this.load();
    if (!projections) {
      new Notice(this.statusMessage());
      return;
    }
    const { summary } = projections;
    new Notice(
      `criv ${summary.schema}: ${summary.node_count} nodes, ${summary.edge_count} edges, ${summary.source_count} source files`,
    );
  }

  async load(): Promise<CrivInitialProjections | null> {
    const status = await this.stream.loaded();
    return status.kind === "ready" ? status.projections : null;
  }

  current(): CrivInitialProjections | null {
    const status = this.stream.status;
    return status.kind === "ready" ? status.projections : null;
  }

  statusMessage(): string {
    return crivStatusMessage(this.stream.status);
  }

  sourceResolver(): SourceResolver {
    return {
      lookupSourceTarget: (target) => this.stream.lookupSourceTarget(target),
      sourceEntry: (path) => this.sourcesByPath.get(path),
    };
  }

  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[] {
    return this.stream.suggestSelectors(query, limit);
  }

  dispose(): void {
    this.stream.dispose();
    this.sourcesByPath.clear();
  }

  private observe(status: CrivStateStatus<string>): void {
    this.sourcesByPath =
      status.kind === "ready"
        ? new Map(status.projections.sources.map((source) => [source.path, source]))
        : new Map();
    if (status.kind === "unavailable" && !this.wasmFailureNotified) {
      this.wasmFailureNotified = true;
      new Notice(status.message);
    }
  }
}

class ObsidianStateHost implements CrivStateHost<string> {
  private lastRead: StateFileToken | null = null;

  constructor(
    private readonly app: App,
    private readonly statePath: () => string,
  ) {}

  async locate(): Promise<string> {
    const path = safeVaultPath(this.statePath());
    if (!path) {
      throw new CrivStateMissingError(`Invalid criv state path ${this.statePath()}.`);
    }
    return path;
  }

  async read(path: string): Promise<string> {
    this.lastRead = await this.token(path);
    return readCrivState(path, () => this.app.vault.adapter.read(path));
  }

  async changed(): Promise<boolean> {
    const path = safeVaultPath(this.statePath());
    if (!path) {
      return false;
    }
    const token = await this.token(path);
    return token?.mtime !== this.lastRead?.mtime || token?.size !== this.lastRead?.size;
  }

  private async token(path: string): Promise<StateFileToken | null> {
    const stat = await this.app.vault.adapter.stat(path).catch(() => null);
    return stat ? { mtime: stat.mtime, size: stat.size } : null;
  }
}

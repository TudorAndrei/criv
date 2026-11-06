import {
  CrivWasmLoadError,
  type CrivInitialProjections,
  type CrivSelectorSuggestion,
  type CrivSourceTargetLookupResult,
} from "./contract.ts";
import { LoadedRevisionOwner } from "./revisionOwner.ts";
import type { CrivLoadedState, CrivStateModule } from "./stateModule.ts";
import { errorMessage } from "./values.ts";

export type CrivStateStatus<Source> =
  | { generation: number; kind: "loading" }
  | { generation: number; kind: "ready"; source: Source; projections: CrivInitialProjections }
  | {
      generation: number;
      kind: "missing" | "invalid" | "unavailable";
      source?: Source;
      message: string;
    };

/** Where an editor finds and reads State. `Source` names the State location. */
export interface CrivStateHost<Source> {
  /** Throw `CrivStateMissingError` when the editor has no State location. */
  locate(): Promise<Source>;
  /** Throw `CrivStateMissingError` when the State text cannot be read. */
  read(source: Source): Promise<string>;
  /** Called once when the stream is disposed. */
  dispose?(): void;
}

export interface Disposable {
  dispose(): void;
}

export class CrivStateMissingError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "CrivStateMissingError";
  }
}

/**
 * Owns one loaded State revision for an editor workspace and publishes every
 * change as a status with a monotonic generation. Only the latest refresh can
 * publish; a superseded or late revision is disposed.
 */
export class CrivStateStream<Source> implements Disposable {
  private readonly host: CrivStateHost<Source>;
  private readonly stateModule: CrivStateModule;
  private readonly revisions = new LoadedRevisionOwner<CrivLoadedState>();
  private readonly listeners = new Set<(status: CrivStateStatus<Source>) => void>();
  private statusValue: CrivStateStatus<Source> = { generation: 0, kind: "loading" };
  private generation = 0;
  private disposed = false;

  constructor(host: CrivStateHost<Source>, stateModule: CrivStateModule) {
    this.host = host;
    this.stateModule = stateModule;
  }

  get status(): CrivStateStatus<Source> {
    return this.statusValue;
  }

  onDidChange(listener: (status: CrivStateStatus<Source>) => void): Disposable {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  async refresh(): Promise<CrivStateStatus<Source>> {
    if (this.disposed) {
      return this.statusValue;
    }
    const generation = ++this.generation;
    if (!this.revisions.current) {
      this.publish({ generation, kind: "loading" });
    }

    let source: Source | undefined;
    const result = await this.revisions.replace(
      async (attempt) => {
        source = await this.host.locate();
        attempt.assertCurrent();
        const raw = await this.host.read(source);
        attempt.assertCurrent();
        return this.stateModule.loadState(raw);
      },
      (revision) => revision.initialProjections(),
    );

    switch (result.kind) {
      case "superseded":
      case "closed":
        return this.statusValue;
      case "committed":
        return this.publish({
          generation,
          kind: "ready",
          source: source as Source,
          projections: result.value,
        });
      case "failed":
        return this.publish({ generation, source, ...failureStatus(result.error) });
    }
  }

  lookupSourceTarget(target: string): CrivSourceTargetLookupResult {
    return this.revisions.current?.lookupSourceTarget(target) ?? { kind: "unresolved" };
  }

  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[] {
    return this.revisions.current?.suggestSelectors(query, limit) ?? [];
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.host.dispose?.();
    this.revisions.dispose();
    this.listeners.clear();
  }

  private publish(status: CrivStateStatus<Source>): CrivStateStatus<Source> {
    this.statusValue = status;
    for (const listener of this.listeners) {
      listener(status);
    }
    return status;
  }
}

function failureStatus(error: unknown): {
  kind: "missing" | "invalid" | "unavailable";
  message: string;
} {
  if (error instanceof CrivStateMissingError) {
    return { kind: "missing", message: error.message };
  }
  if (error instanceof CrivWasmLoadError) {
    return { kind: "unavailable", message: error.message };
  }
  return { kind: "invalid", message: `Invalid criv State: ${errorMessage(error)}` };
}

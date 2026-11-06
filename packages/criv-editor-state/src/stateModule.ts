import {
  CRIV_STATE_JSON_INVALID,
  CRIV_STATE_SCHEMA_UNSUPPORTED,
  CrivLoadedStateDisposedError,
  CrivStateContractError,
  CrivWasmLoadError,
  type CrivInitialProjections,
  type CrivSelectorSuggestion,
  type CrivSourceTargetLookupResult,
} from "./contract.ts";
import { isRecord } from "./values.ts";

export interface CrivLoadedState {
  initialProjections(): CrivInitialProjections;
  lookupSourceTarget(target: string): CrivSourceTargetLookupResult;
  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[];
  dispose(): void;
}

export interface CrivStateModule {
  loadState(raw: string): Promise<CrivLoadedState>;
}

interface WasmLoadedState {
  initialProjections(): CrivInitialProjections;
  lookupSourceTarget(target: string): CrivSourceTargetLookupResult;
  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[];
  free(): void;
}

interface WasmModule {
  LoadedState: new (raw: string) => WasmLoadedState;
}

const stateContractErrorCodes: ReadonlySet<unknown> = new Set([
  CRIV_STATE_JSON_INVALID,
  CRIV_STATE_SCHEMA_UNSUPPORTED,
]);

/**
 * Loads the generated Wasm module once and returns State revisions from it.
 * A failed module load is cached and rejects every later load with the same
 * `CrivWasmLoadError`, whose message is `unavailableMessage`.
 */
export function createCrivStateModule(
  loadModule: () => Promise<unknown>,
  unavailableMessage: string,
): CrivStateModule {
  let moduleLoad: Promise<WasmModule> | undefined;

  const loadWasm = (): Promise<WasmModule> => {
    moduleLoad ??= Promise.resolve()
      .then(loadModule)
      .then(requireWasmModule)
      .catch((error: unknown) => {
        throw new CrivWasmLoadError(unavailableMessage, { cause: error });
      });
    return moduleLoad;
  };

  return {
    async loadState(raw) {
      const wasm = await loadWasm();
      let loaded: WasmLoadedState;
      try {
        loaded = new wasm.LoadedState(raw);
      } catch (error) {
        throw stateContractError(error) ?? error;
      }
      try {
        return new LoadedState(loaded, loaded.initialProjections());
      } catch (error) {
        loaded.free();
        throw error;
      }
    },
  };
}

function stateContractError(error: unknown): CrivStateContractError | undefined {
  if (!(error instanceof Error) || !isRecord(error) || !stateContractErrorCodes.has(error.code)) {
    return undefined;
  }
  return new CrivStateContractError(error.code as CrivStateContractError["code"], error.message);
}

function requireWasmModule(value: unknown): WasmModule {
  if (!isRecord(value) || typeof value.LoadedState !== "function") {
    throw new Error("criv Wasm module is missing export LoadedState");
  }
  return value as unknown as WasmModule;
}

class LoadedState implements CrivLoadedState {
  private readonly loaded: WasmLoadedState;
  private readonly projections: CrivInitialProjections;
  private disposed = false;

  constructor(loaded: WasmLoadedState, projections: CrivInitialProjections) {
    this.loaded = loaded;
    this.projections = projections;
  }

  initialProjections(): CrivInitialProjections {
    this.assertAvailable();
    return this.projections;
  }

  lookupSourceTarget(target: string): CrivSourceTargetLookupResult {
    this.assertAvailable();
    return this.loaded.lookupSourceTarget(target);
  }

  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[] {
    this.assertAvailable();
    return this.loaded.suggestSelectors(query, limit);
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.loaded.free();
  }

  private assertAvailable(): void {
    if (this.disposed) {
      throw new CrivLoadedStateDisposedError();
    }
  }
}

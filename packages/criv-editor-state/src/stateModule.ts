import {
  CrivLoadedStateDisposedError,
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

const WASM_UNAVAILABLE_MESSAGE =
  "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload the editor.";

/**
 * Loads the generated Wasm module once and returns State revisions from it.
 * A failed module load is cached and rejects every later load with the same
 * `CrivWasmLoadError`. A rejected State document keeps the Wasm `Error`, whose
 * `code` is `CRIV_STATE_JSON_INVALID` or `CRIV_STATE_SCHEMA_UNSUPPORTED`.
 */
export function createCrivStateModule(loadModule: () => Promise<unknown>): CrivStateModule {
  let moduleLoad: Promise<WasmModule> | undefined;

  const loadWasm = (): Promise<WasmModule> => {
    moduleLoad ??= Promise.resolve()
      .then(loadModule)
      .then(requireWasmModule)
      .catch((error: unknown) => {
        throw new CrivWasmLoadError(WASM_UNAVAILABLE_MESSAGE, { cause: error });
      });
    return moduleLoad;
  };

  return {
    async loadState(raw) {
      const wasm = await loadWasm();
      const loaded = new wasm.LoadedState(raw);
      try {
        return new LoadedState(loaded, loaded.initialProjections());
      } catch (error) {
        loaded.free();
        throw error;
      }
    },
  };
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

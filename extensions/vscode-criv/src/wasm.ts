export interface CrivStateSummary {
  schema: string;
  node_count: number;
  edge_count: number;
  source_count: number;
  pattern_count: number;
  first_node_id?: string;
  first_edge?: string;
  first_source_path?: string;
}

export interface CrivSourceEntry {
  path: string;
  mime?: string;
  frecency: number;
}

export interface CrivGraphNode {
  id: string;
  kind: string;
  label: string;
  path?: string;
  source_target?: string;
  line_range?: string;
}

export interface CrivSelectorSuggestion {
  target: string;
  label: string;
  kind: string;
  path: string;
  detail: string;
}

export interface CrivValidatedState {
  schema?: unknown;
  graph?: {
    nodes?: unknown[];
    edges?: unknown[];
  };
  "registered-patterns"?: unknown[];
  "source-index"?: unknown[];
  patterns?: Record<string, unknown[]>;
  [key: string]: unknown;
}

export interface CrivInitialProjections {
  state: CrivValidatedState;
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  nodes: CrivGraphNode[];
}

type CrivWasmLoadedState = {
  initialProjections(): CrivInitialProjections;
  lookupNode(target: string): CrivGraphNode | undefined;
  suggestSelectors(query: string, limit: number): CrivSelectorSuggestion[];
  free(): void;
};

type CrivWasmModule = {
  LoadedState: new (raw: string) => CrivWasmLoadedState;
};

export const CRIV_WASM_LOAD_ERROR = "criv-wasm-unavailable";
export const CRIV_LOADED_STATE_DISPOSED = "criv-loaded-state-disposed";

export class CrivWasmLoadError extends Error {
  readonly code = CRIV_WASM_LOAD_ERROR;

  constructor(cause: unknown) {
    super(
      "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload the editor.",
      { cause },
    );
    this.name = "CrivWasmLoadError";
  }
}

export class CrivLoadedStateDisposedError extends Error {
  readonly code = CRIV_LOADED_STATE_DISPOSED;

  constructor() {
    super("The loaded criv State revision was disposed.");
    this.name = "CrivLoadedStateDisposedError";
  }
}

export interface CrivLoadedState {
  initialProjections(): CrivInitialProjections;
  lookupNode(target: string): CrivGraphNode | undefined;
  suggestSelectors(query: string, limit?: number): CrivSelectorSuggestion[];
  dispose(): void;
}

export interface CrivWasmBridge {
  loadState(raw: string): Promise<CrivLoadedState>;
}

export type CrivWasmLoader = () => Promise<unknown>;

const bridge = createCrivWasmBridge(() => import("../pkg/criv_wasm.js"));

export function createCrivWasmBridge(loader: CrivWasmLoader): CrivWasmBridge {
  let wasmModule: Promise<CrivWasmModule> | undefined;
  const loadWasm = (): Promise<CrivWasmModule> => {
    wasmModule ??= loader()
      .then(requireCrivWasmModule)
      .catch((error: unknown) => {
        throw error instanceof CrivWasmLoadError ? error : new CrivWasmLoadError(error);
      });
    return wasmModule;
  };

  return {
    async loadState(raw) {
      const wasm = await loadWasm();
      const loaded = new wasm.LoadedState(raw);
      try {
        return new LoadedStateAdapter(loaded);
      } catch (error) {
        loaded.free();
        throw error;
      }
    },
  };
}

export function loadState(raw: string): Promise<CrivLoadedState> {
  return bridge.loadState(raw);
}

function requireCrivWasmModule(value: unknown): CrivWasmModule {
  if (!isRecord(value)) {
    throw new Error("criv Wasm module did not export an object");
  }
  if (typeof value.LoadedState !== "function") {
    throw new Error("criv Wasm module is missing export LoadedState");
  }
  return value as CrivWasmModule;
}

class LoadedStateAdapter implements CrivLoadedState {
  private readonly projections: CrivInitialProjections;
  private disposed = false;

  constructor(private readonly loaded: CrivWasmLoadedState) {
    this.projections = loaded.initialProjections();
  }

  initialProjections(): CrivInitialProjections {
    this.assertAvailable();
    return this.projections;
  }

  lookupNode(target: string): CrivGraphNode | undefined {
    this.assertAvailable();
    return this.loaded.lookupNode(target);
  }

  suggestSelectors(query: string, limit = 20): CrivSelectorSuggestion[] {
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

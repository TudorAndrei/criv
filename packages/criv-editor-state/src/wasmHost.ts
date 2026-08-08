export const CRIV_WASM_LOAD_ERROR = "criv-wasm-unavailable";
export const CRIV_LOADED_STATE_DISPOSED = "criv-loaded-state-disposed";

export class CrivWasmLoadError extends Error {
  readonly code = CRIV_WASM_LOAD_ERROR;

  constructor(message: string, cause: unknown) {
    super(message);
    this.name = "CrivWasmLoadError";
    (this as Error & { cause?: unknown }).cause = cause;
  }
}

export class CrivLoadedStateDisposedError extends Error {
  readonly code = CRIV_LOADED_STATE_DISPOSED;

  constructor() {
    super("The loaded criv State revision was disposed.");
    this.name = "CrivLoadedStateDisposedError";
  }
}

export interface CrivLoadedState<Projections, Node, Suggestion> {
  initialProjections(): Projections;
  lookupNode(target: string): Node;
  suggestSelectors(query: string, limit?: number): Suggestion[];
  dispose(): void;
}

export interface CrivWasmHost<Projections, Node, Suggestion> {
  loadState(raw: string): Promise<CrivLoadedState<Projections, Node, Suggestion>>;
}

export type CrivWasmModuleLoader = () => Promise<unknown>;

type CrivWasmLoadedState<Projections, Node, Suggestion> = {
  initialProjections(): Projections;
  lookupNode(target: string): Node;
  suggestSelectors(query: string, limit: number): Suggestion[];
  free(): void;
};

type CrivWasmModule<Projections, Node, Suggestion> = {
  LoadedState: new (raw: string) => CrivWasmLoadedState<Projections, Node, Suggestion>;
};

export function createCrivWasmHost<Projections, Node, Suggestion>(
  loadModule: CrivWasmModuleLoader,
  unavailableMessage: string,
): CrivWasmHost<Projections, Node, Suggestion> {
  let moduleLoad: Promise<CrivWasmModule<Projections, Node, Suggestion>> | undefined;

  const loadWasm = (): Promise<CrivWasmModule<Projections, Node, Suggestion>> => {
    moduleLoad ??= Promise.resolve()
      .then(loadModule)
      .then(requireCrivWasmModule<Projections, Node, Suggestion>)
      .catch((error: unknown) => {
        throw error instanceof CrivWasmLoadError
          ? error
          : new CrivWasmLoadError(unavailableMessage, error);
      });
    return moduleLoad;
  };

  return {
    async loadState(raw) {
      const wasm = await loadWasm();
      const loaded = new wasm.LoadedState(raw);
      try {
        const projections = loaded.initialProjections();
        return new LoadedStateAdapter(loaded, projections);
      } catch (error) {
        loaded.free();
        throw error;
      }
    },
  };
}

function requireCrivWasmModule<Projections, Node, Suggestion>(
  value: unknown,
): CrivWasmModule<Projections, Node, Suggestion> {
  if (!isRecord(value)) {
    throw new Error("criv Wasm module did not export an object");
  }
  if (typeof value.LoadedState !== "function") {
    throw new Error("criv Wasm module is missing export LoadedState");
  }
  return value as CrivWasmModule<Projections, Node, Suggestion>;
}

class LoadedStateAdapter<Projections, Node, Suggestion>
  implements CrivLoadedState<Projections, Node, Suggestion>
{
  private readonly loaded: CrivWasmLoadedState<Projections, Node, Suggestion>;
  private readonly projections: Projections;
  private disposed = false;

  constructor(
    loaded: CrivWasmLoadedState<Projections, Node, Suggestion>,
    projections: Projections,
  ) {
    this.loaded = loaded;
    this.projections = projections;
  }

  initialProjections(): Projections {
    this.assertAvailable();
    return this.projections;
  }

  lookupNode(target: string): Node {
    this.assertAvailable();
    return this.loaded.lookupNode(target);
  }

  suggestSelectors(query: string, limit = 20): Suggestion[] {
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

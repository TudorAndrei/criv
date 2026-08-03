import type { CrivState, SourceIndexEntry } from "./core";

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

export interface CrivSelectorSuggestion {
  target: string;
  label: string;
  kind: string;
  path: string;
  detail: string;
}

type CrivWasmModule = {
  validated_state(raw: string): CrivState;
  summarize_state(raw: string): CrivStateSummary;
  source_entries(raw: string): SourceIndexEntry[];
  graph_nodes(raw: string): unknown[];
  suggest_source_selectors(raw: string, query: string, limit: number): CrivSelectorSuggestion[];
  lookup_graph_node(raw: string, target: string): unknown;
};

export const CRIV_WASM_LOAD_ERROR = "criv-wasm-unavailable";

export class CrivWasmLoadError extends Error {
  readonly code = CRIV_WASM_LOAD_ERROR;

  constructor(cause: unknown) {
    super(
      "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload Obsidian.",
    );
    this.name = "CrivWasmLoadError";
    (this as Error & { cause?: unknown }).cause = cause;
  }
}

export interface CrivWasmBridge {
  validatedState(raw: string): Promise<CrivState>;
  summarizeState(raw: string): Promise<CrivStateSummary>;
  sourceEntries(raw: string): Promise<SourceIndexEntry[]>;
  suggestSourceSelectors(
    raw: string,
    query: string,
    limit: number,
  ): Promise<CrivSelectorSuggestion[]>;
}

export type CrivWasmLoader = () => Promise<unknown>;

const loadPackagedWasm = () =>
  // @ts-expect-error wasm-pack creates this external runtime before the plugin is distributed.
  import("./pkg/criv_wasm.js");
const bridge = createCrivWasmBridge(loadPackagedWasm);

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
    async validatedState(raw) {
      return (await loadWasm()).validated_state(raw);
    },
    async summarizeState(raw) {
      return (await loadWasm()).summarize_state(raw);
    },
    async sourceEntries(raw) {
      return (await loadWasm()).source_entries(raw);
    },
    async suggestSourceSelectors(raw, query, limit) {
      return (await loadWasm()).suggest_source_selectors(raw, query, limit);
    },
  };
}

export function validatedState(raw: string): Promise<CrivState> {
  return bridge.validatedState(raw);
}

export function summarizeState(raw: string): Promise<CrivStateSummary> {
  return bridge.summarizeState(raw);
}

export function sourceEntries(raw: string): Promise<SourceIndexEntry[]> {
  return bridge.sourceEntries(raw);
}

export function suggestSourceSelectors(
  raw: string,
  query: string,
  limit: number,
): Promise<CrivSelectorSuggestion[]> {
  return bridge.suggestSourceSelectors(raw, query, limit);
}

function requireCrivWasmModule(value: unknown): CrivWasmModule {
  if (!isRecord(value)) {
    throw new Error("criv Wasm module did not export an object");
  }
  for (const name of [
    "validated_state",
    "summarize_state",
    "source_entries",
    "graph_nodes",
    "suggest_source_selectors",
    "lookup_graph_node",
  ]) {
    if (typeof value[name] !== "function") {
      throw new Error(`criv Wasm module is missing export ${name}`);
    }
  }
  return value as CrivWasmModule;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

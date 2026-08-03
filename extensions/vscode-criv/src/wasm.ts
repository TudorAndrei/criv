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

type CrivWasmModule = {
  validated_state(raw: string): CrivValidatedState;
  summarize_state(raw: string): CrivStateSummary;
  source_entries(raw: string): CrivSourceEntry[];
  graph_nodes(raw: string): CrivGraphNode[];
  suggest_source_selectors(raw: string, query: string, limit: number): CrivSelectorSuggestion[];
  lookup_graph_node(raw: string, target: string): CrivGraphNode | undefined;
};

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

export const CRIV_WASM_LOAD_ERROR = "criv-wasm-unavailable";

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

export interface CrivWasmBridge {
  validatedState(raw: string): Promise<CrivValidatedState>;
  summarizeState(raw: string): Promise<CrivStateSummary>;
  sourceEntries(raw: string): Promise<CrivSourceEntry[]>;
  graphNodes(raw: string): Promise<CrivGraphNode[]>;
  suggestSourceSelectors(
    raw: string,
    query: string,
    limit?: number,
  ): Promise<CrivSelectorSuggestion[]>;
  lookupGraphNode(raw: string, target: string): Promise<CrivGraphNode | undefined>;
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
    async validatedState(raw) {
      return (await loadWasm()).validated_state(raw);
    },
    async summarizeState(raw) {
      return (await loadWasm()).summarize_state(raw);
    },
    async sourceEntries(raw) {
      return (await loadWasm()).source_entries(raw);
    },
    async graphNodes(raw) {
      return (await loadWasm()).graph_nodes(raw);
    },
    async suggestSourceSelectors(raw, query, limit = 20) {
      return (await loadWasm()).suggest_source_selectors(raw, query, limit);
    },
    async lookupGraphNode(raw, target) {
      return (await loadWasm()).lookup_graph_node(raw, target);
    },
  };
}

export function validatedState(raw: string): Promise<CrivValidatedState> {
  return bridge.validatedState(raw);
}

export async function summarizeState(raw: string): Promise<CrivStateSummary> {
  return bridge.summarizeState(raw);
}

export async function sourceEntries(raw: string): Promise<CrivSourceEntry[]> {
  return bridge.sourceEntries(raw);
}

export async function graphNodes(raw: string): Promise<CrivGraphNode[]> {
  return bridge.graphNodes(raw);
}

export async function suggestSourceSelectors(
  raw: string,
  query: string,
  limit = 20,
): Promise<CrivSelectorSuggestion[]> {
  return bridge.suggestSourceSelectors(raw, query, limit);
}

export async function lookupGraphNode(
  raw: string,
  target: string,
): Promise<CrivGraphNode | undefined> {
  return bridge.lookupGraphNode(raw, target);
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

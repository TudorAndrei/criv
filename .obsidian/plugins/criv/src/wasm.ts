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
  summarize_state(raw: string): CrivStateSummary;
  suggest_source_selectors(raw: string, query: string, limit: number): CrivSelectorSuggestion[];
};

let wasmModule: Promise<CrivWasmModule | null> | null = null;
const WASM_RUNTIME_PATH = "./pkg/criv_wasm.js";

export async function summarizeState(raw: string): Promise<CrivStateSummary> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.summarize_state(raw);
  }

  const state = JSON.parse(raw);
  const sourcePaths = uniqueSourcePaths(state["source-index"]);
  return {
    schema: state.schema,
    node_count: Array.isArray(state.graph?.nodes) ? state.graph.nodes.length : 0,
    edge_count: Array.isArray(state.graph?.edges) ? state.graph.edges.length : 0,
    source_count: sourcePaths.length,
    pattern_count: Array.isArray(state["registered-patterns"])
      ? state["registered-patterns"].length
      : 0,
    first_node_id: state.graph?.nodes?.[0]?.id,
    first_edge: state.graph?.edges?.[0]
      ? `${state.graph.edges[0].from}:${state.graph.edges[0].kind}:${state.graph.edges[0].to}`
      : undefined,
    first_source_path: sourcePaths[0],
  };
}

export async function suggestSourceSelectors(
  raw: string,
  query: string,
  limit: number,
): Promise<CrivSelectorSuggestion[] | null> {
  const wasm = await loadWasm();
  if (!wasm) {
    return null;
  }
  return wasm.suggest_source_selectors(raw, query, limit);
}

function uniqueSourcePaths(sourceIndex: unknown): string[] {
  if (!Array.isArray(sourceIndex)) {
    return [];
  }

  const seen = new Set<string>();
  const paths: string[] = [];
  for (const entry of sourceIndex) {
    const path = entry && typeof entry === "object" ? (entry as { path?: unknown }).path : null;
    if (typeof path !== "string" || !path || seen.has(path)) {
      continue;
    }
    seen.add(path);
    paths.push(path);
  }
  return paths;
}

async function loadWasm(): Promise<CrivWasmModule | null> {
  if (!wasmModule) {
    wasmModule = import(WASM_RUNTIME_PATH)
      .then((module) => module as CrivWasmModule)
      .catch(() => null);
  }
  return wasmModule;
}

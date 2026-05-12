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

type CrivWasmModule = {
  summarize_state(raw: string): CrivStateSummary;
};

let wasmModule: Promise<CrivWasmModule | null> | null = null;

export async function summarizeState(raw: string): Promise<CrivStateSummary> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.summarize_state(raw);
  }

  const state = JSON.parse(raw);
  return {
    schema: state.schema,
    node_count: Array.isArray(state.graph?.nodes) ? state.graph.nodes.length : 0,
    edge_count: Array.isArray(state.graph?.edges) ? state.graph.edges.length : 0,
    source_count: Array.isArray(state["source-index"]) ? state["source-index"].length : 0,
    pattern_count: Array.isArray(state["registered-patterns"]) ? state["registered-patterns"].length : 0,
    first_node_id: state.graph?.nodes?.[0]?.id,
    first_edge: state.graph?.edges?.[0]
      ? `${state.graph.edges[0].from}:${state.graph.edges[0].kind}:${state.graph.edges[0].to}`
      : undefined,
    first_source_path: state["source-index"]?.[0]?.path,
  };
}

async function loadWasm(): Promise<CrivWasmModule | null> {
  if (!wasmModule) {
    wasmModule = import("../pkg/criv_wasm.js")
      .then((module) => module as CrivWasmModule)
      .catch(() => null);
  }
  return wasmModule;
}

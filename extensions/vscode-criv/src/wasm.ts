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
  summarize_state(raw: string): CrivStateSummary;
  source_entries(raw: string): CrivSourceEntry[];
  graph_nodes(raw: string): CrivGraphNode[];
  suggest_source_selectors(raw: string, query: string, limit: number): CrivSelectorSuggestion[];
  lookup_graph_node(raw: string, target: string): CrivGraphNode | undefined;
};

type JsonRecord = Record<string, unknown>;

interface CrivStateJson {
  schema?: unknown;
  graph?: {
    nodes?: JsonRecord[];
    edges?: JsonRecord[];
  };
  "registered-patterns"?: unknown[];
  "source-index"?: JsonRecord[];
}

let wasmModule: Promise<CrivWasmModule | null> | null = null;
const WASM_RUNTIME_PATH = "../pkg/criv_wasm.js";

export async function summarizeState(raw: string): Promise<CrivStateSummary> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.summarize_state(raw);
  }
  return fallbackSummary(raw);
}

export async function sourceEntries(raw: string): Promise<CrivSourceEntry[]> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.source_entries(raw);
  }
  return fallbackSourceEntries(raw);
}

export async function graphNodes(raw: string): Promise<CrivGraphNode[]> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.graph_nodes(raw);
  }
  return fallbackGraphNodes(raw);
}

export async function suggestSourceSelectors(
  raw: string,
  query: string,
  limit = 20,
): Promise<CrivSelectorSuggestion[]> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.suggest_source_selectors(raw, query, limit);
  }
  return fallbackSourceSelectors(raw, query, limit);
}

export async function lookupGraphNode(
  raw: string,
  target: string,
): Promise<CrivGraphNode | undefined> {
  const wasm = await loadWasm();
  if (wasm) {
    return wasm.lookup_graph_node(raw, target);
  }
  return fallbackGraphNodes(raw).find(
    (node) => node.id === target || node.source_target === target || node.path === target,
  );
}

async function loadWasm(): Promise<CrivWasmModule | null> {
  if (!wasmModule) {
    wasmModule = import(WASM_RUNTIME_PATH)
      .then((module) => module as CrivWasmModule)
      .catch(() => null);
  }
  return wasmModule;
}

function fallbackSummary(raw: string): CrivStateSummary {
  const state = parseState(raw);
  const sourcePaths = fallbackSourceEntries(raw).map((entry) => entry.path);
  const firstEdge = state.graph?.edges?.[0];
  return {
    schema: stringValue(state.schema),
    node_count: Array.isArray(state.graph?.nodes) ? state.graph.nodes.length : 0,
    edge_count: Array.isArray(state.graph?.edges) ? state.graph.edges.length : 0,
    source_count: sourcePaths.length,
    pattern_count: Array.isArray(state["registered-patterns"])
      ? state["registered-patterns"].length
      : 0,
    first_node_id: stringValue(state.graph?.nodes?.[0]?.id) || undefined,
    first_edge: firstEdge
      ? `${stringValue(firstEdge.from)}:${stringValue(firstEdge.kind)}:${stringValue(firstEdge.to)}`
      : undefined,
    first_source_path: sourcePaths[0],
  };
}

function fallbackSourceEntries(raw: string): CrivSourceEntry[] {
  const state = parseState(raw);
  const seen = new Set<string>();
  const sourceIndex = state["source-index"] ?? [];
  const entries: CrivSourceEntry[] = [];
  for (const entry of sourceIndex) {
    const path = stringValue(entry?.path);
    if (!path || seen.has(path)) {
      continue;
    }
    seen.add(path);
    entries.push({
      path,
      mime: stringValue(entry?.mime) || undefined,
      frecency: numberValue(entry?.frecency),
    });
  }
  return entries;
}

function fallbackGraphNodes(raw: string): CrivGraphNode[] {
  const state = parseState(raw);
  const nodes = state.graph?.nodes ?? [];
  return nodes.map((node) => {
    const id = stringValue(node.id);
    const path = stringValue(node.path) || undefined;
    return {
      id,
      kind: stringValue(node.kind),
      label: stringValue(node.label) || id,
      path,
      source_target: sourceTarget(id),
      line_range: path ? lineRange(path) : undefined,
    };
  });
}

function fallbackSourceSelectors(
  raw: string,
  query: string,
  limit: number,
): CrivSelectorSuggestion[] {
  const cleanQuery = query.trim().toLowerCase();
  const seen = new Set<string>();
  const suggestions: CrivSelectorSuggestion[] = [];

  for (const entry of fallbackSourceEntries(raw)) {
    if (!matches(entry.path, cleanQuery) || seen.has(entry.path)) {
      continue;
    }
    seen.add(entry.path);
    suggestions.push({
      target: entry.path,
      label: entry.path,
      kind: "file",
      path: entry.path,
      detail: "file",
    });
  }

  for (const node of fallbackGraphNodes(raw)) {
    const target = node.source_target;
    if (!target?.includes("#") || !matches(target, cleanQuery) || seen.has(target)) {
      continue;
    }
    seen.add(target);
    suggestions.push({
      target,
      label: node.label,
      kind: node.kind,
      path: node.path ?? "",
      detail: node.id,
    });
  }

  return suggestions
    .sort((left, right) => rank(left.target, cleanQuery) - rank(right.target, cleanQuery))
    .slice(0, limit);
}

function parseState(raw: string): CrivStateJson {
  return JSON.parse(raw) as CrivStateJson;
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function numberValue(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function sourceTarget(id: string): string | undefined {
  return id.startsWith("symbol:") || id.startsWith("code:")
    ? id.slice(id.indexOf(":") + 1)
    : undefined;
}

function lineRange(path: string): string | undefined {
  const marker = "#L";
  const index = path.indexOf(marker);
  return index === -1 ? undefined : path.slice(index + 1);
}

function matches(candidate: string, query: string): boolean {
  return !query || candidate.toLowerCase().includes(query);
}

function rank(candidate: string, query: string): number {
  if (!query) {
    return 0;
  }
  const lower = candidate.toLowerCase();
  if (lower === query) {
    return 0;
  }
  return lower.startsWith(query) ? 1 : 2;
}

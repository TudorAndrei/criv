import {
  createCrivWasmHost,
  type CrivLoadedState as SharedLoadedState,
  type CrivWasmHost,
  type CrivWasmModuleLoader,
} from "@criv/editor-state";

export {
  CRIV_LOADED_STATE_DISPOSED,
  CRIV_WASM_LOAD_ERROR,
  CrivLoadedStateDisposedError,
  CrivWasmLoadError,
} from "@criv/editor-state";

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

export type CrivLoadedState = SharedLoadedState<
  CrivInitialProjections,
  CrivGraphNode | undefined,
  CrivSelectorSuggestion
>;
export type CrivWasmBridge = CrivWasmHost<
  CrivInitialProjections,
  CrivGraphNode | undefined,
  CrivSelectorSuggestion
>;
export type CrivWasmLoader = CrivWasmModuleLoader;

const unavailableMessage =
  "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload the editor.";
const loadPackagedWasm: CrivWasmLoader = () => import("../pkg/criv_wasm.js");
const bridge = createCrivWasmBridge();

export function createCrivWasmBridge(loader: CrivWasmLoader = loadPackagedWasm): CrivWasmBridge {
  return createCrivWasmHost(loader, unavailableMessage);
}

export function loadState(raw: string): Promise<CrivLoadedState> {
  return bridge.loadState(raw);
}

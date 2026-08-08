import {
  createCrivWasmHost,
  type CrivLoadedState as SharedLoadedState,
  type CrivWasmHost,
  type CrivWasmModuleLoader,
} from "@criv/editor-state";
import type { CrivState, SourceIndexEntry } from "./core";

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

export interface CrivSelectorSuggestion {
  target: string;
  label: string;
  kind: string;
  path: string;
  detail: string;
}

export interface CrivInitialProjections {
  state: CrivState;
  summary: CrivStateSummary;
  sources: SourceIndexEntry[];
  nodes: unknown[];
}

export type CrivLoadedState = SharedLoadedState<
  CrivInitialProjections,
  unknown,
  CrivSelectorSuggestion
>;
export type CrivWasmBridge = CrivWasmHost<
  CrivInitialProjections,
  unknown,
  CrivSelectorSuggestion
>;
export type CrivWasmLoader = CrivWasmModuleLoader;

const unavailableMessage =
  "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload Obsidian.";
const loadPackagedWasm: CrivWasmLoader = () =>
  // @ts-expect-error wasm-pack creates this external runtime before the plugin is distributed.
  import("./pkg/criv_wasm.js");
const bridge = createCrivWasmBridge();

export function createCrivWasmBridge(loader: CrivWasmLoader = loadPackagedWasm): CrivWasmBridge {
  return createCrivWasmHost(loader, unavailableMessage);
}

export function loadState(raw: string): Promise<CrivLoadedState> {
  return bridge.loadState(raw);
}

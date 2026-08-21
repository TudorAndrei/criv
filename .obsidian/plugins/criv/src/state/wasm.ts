import {
  createCrivWasmHost,
  type CrivLoadedState as SharedLoadedState,
  type CrivWasmHost,
  type CrivWasmModuleLoader,
} from "@criv/editor-state";
import type {
  CrivNode,
  AssetIndexEntry,
  CrivSourceTargetLookupResult,
  CrivState,
  SourceIndexEntry,
} from "../source/model";

export {
  CRIV_LIKEC4_ARCHITECTURE_INVALID,
  CRIV_LIKEC4_MODEL_INVALID,
  CRIV_LIKEC4_PROTOCOL_UNSUPPORTED,
  CRIV_LIKEC4_VERSION_UNSUPPORTED,
  CRIV_LOADED_STATE_DISPOSED,
  CRIV_STATE_JSON_INVALID,
  CRIV_STATE_SCHEMA_UNSUPPORTED,
  CRIV_WASM_LOAD_ERROR,
  CrivLoadedStateDisposedError,
  CrivStateContractError,
  CrivWasmLoadError,
} from "@criv/editor-state";

export interface CrivStateSummary {
  schema: string;
  node_count: number;
  edge_count: number;
  source_count: number;
  asset_count: number;
  pattern_count: number;
  first_node_id?: string;
  first_edge?: string;
  first_source_path?: string;
  first_asset_path?: string;
}

export interface CrivSelectorSuggestion {
  target: string;
  label: string;
  kind: string;
  path: string;
  detail: string;
}

export interface CrivInitialProjections {
  summary: CrivStateSummary;
  sources: SourceIndexEntry[];
  assets: AssetIndexEntry[];
  nodes: CrivNode[];
  registeredPatterns: CrivState["registeredPatterns"];
  patternMatches: CrivState["patternMatches"];
  architecture?: CrivState["architecture"];
  c4Artifacts: { path: string; label: string; target: string }[];
}

export type CrivLoadedState = SharedLoadedState<
  CrivInitialProjections,
  CrivSourceTargetLookupResult,
  CrivSelectorSuggestion
>;
export type CrivWasmBridge = CrivWasmHost<
  CrivInitialProjections,
  CrivSourceTargetLookupResult,
  CrivSelectorSuggestion
>;
export type CrivWasmLoader = CrivWasmModuleLoader;

const unavailableMessage =
  "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload Obsidian.";
const loadPackagedWasm: CrivWasmLoader = () => import("../../pkg/criv_wasm.js");
const bridge = createCrivWasmBridge();

export function createCrivWasmBridge(loader: CrivWasmLoader = loadPackagedWasm): CrivWasmBridge {
  return createCrivWasmHost(loader, unavailableMessage);
}

export function loadState(raw: string): Promise<CrivLoadedState> {
  return bridge.loadState(raw);
}

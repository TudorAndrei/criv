import {
  createCrivWasmHost,
  type CrivLoadedState as SharedLoadedState,
  type CrivWasmHost,
  type CrivWasmModuleLoader,
} from "@criv/editor-state";
import type { CrivLikeC4Model } from "@criv/likec4/protocol";

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

export interface CrivSourceTargetCandidate {
  canonical_target: string;
  node_id: string;
  kind: string;
  label: string;
}

export type CrivSourceTargetLookupResult =
  | { kind: "resolved"; canonical_target: string; node: CrivGraphNode }
  | { kind: "unresolved" }
  | {
      kind: "ambiguous";
      candidates: CrivSourceTargetCandidate[];
      total_candidate_count: number;
    };

export interface CrivSelectorSuggestion {
  target: string;
  label: string;
  kind: string;
  path: string;
  detail: string;
}

export interface CrivPatternMatch {
  file: string;
  range?: string;
  captures: Record<string, string>;
}

export interface CrivArtifactEntry {
  path: string;
  label: string;
  target: string;
}

export interface CrivInitialProjections {
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  nodes: CrivGraphNode[];
  registeredPatterns: string[];
  patternMatches: Record<string, CrivPatternMatch[]>;
  architecture?: CrivLikeC4Model;
  c4Artifacts: CrivArtifactEntry[];
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
  "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload the editor.";
const loadPackagedWasm: CrivWasmLoader = () => import("../pkg/criv_wasm.js");
const bridge = createCrivWasmBridge();

export function createCrivWasmBridge(loader: CrivWasmLoader = loadPackagedWasm): CrivWasmBridge {
  return createCrivWasmHost(loader, unavailableMessage);
}

export function loadState(raw: string): Promise<CrivLoadedState> {
  return bridge.loadState(raw);
}

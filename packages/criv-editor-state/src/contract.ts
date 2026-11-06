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

export interface CrivSourceEntry {
  path: string;
  mime?: string;
}

export interface CrivAssetEntry {
  path: string;
  mime: string;
  bytes: number;
  hash: string;
}

export interface CrivGraphNode {
  id: string;
  kind: string;
  label: string;
  path?: string;
  source_target?: string;
  line_range?: string;
}

export interface CrivPatternMatch {
  file: string;
  range?: string;
  captures: Record<string, string>;
}

export interface CrivInitialProjections {
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  assets: CrivAssetEntry[];
  nodes: CrivGraphNode[];
  registeredPatterns: string[];
  patternMatches: Record<string, CrivPatternMatch[]>;
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

export const CRIV_WASM_LOAD_ERROR = "criv-wasm-unavailable";
export const CRIV_LOADED_STATE_DISPOSED = "criv-loaded-state-disposed";
export const CRIV_STATE_JSON_INVALID = "criv-state-json-invalid";
export const CRIV_STATE_SCHEMA_UNSUPPORTED = "criv-state-schema-unsupported";

export type CrivStateContractErrorCode =
  | typeof CRIV_STATE_JSON_INVALID
  | typeof CRIV_STATE_SCHEMA_UNSUPPORTED;

export class CrivStateContractError extends Error {
  readonly code: CrivStateContractErrorCode;

  constructor(code: CrivStateContractErrorCode, message: string) {
    super(message);
    this.name = "CrivStateContractError";
    this.code = code;
  }
}

export class CrivWasmLoadError extends Error {
  readonly code = CRIV_WASM_LOAD_ERROR;

  constructor(message: string, options: ErrorOptions) {
    super(message, options);
    this.name = "CrivWasmLoadError";
  }
}

export class CrivLoadedStateDisposedError extends Error {
  readonly code = CRIV_LOADED_STATE_DISPOSED;

  constructor() {
    super("The loaded criv State revision was disposed.");
    this.name = "CrivLoadedStateDisposedError";
  }
}

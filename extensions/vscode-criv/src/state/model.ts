import type {
  CrivAssetEntry,
  CrivGraphNode,
  CrivInitialProjections,
  CrivSourceEntry,
  CrivStateSummary,
} from "./wasm";
export type { CrivAssetEntry } from "./wasm";

export interface CrivStateSnapshot {
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  assets: CrivAssetEntry[];
  graphNodes: CrivGraphNode[];
  registeredPatterns: string[];
}

export function buildStateSnapshot(projections: CrivInitialProjections): CrivStateSnapshot {
  return {
    summary: projections.summary,
    sources: projections.sources,
    assets: projections.assets,
    graphNodes: projections.nodes,
    registeredPatterns: projections.registeredPatterns,
  };
}

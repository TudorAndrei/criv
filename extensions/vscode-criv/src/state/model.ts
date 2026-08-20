import type {
  CrivArtifactEntry,
  CrivAssetEntry,
  CrivGraphNode,
  CrivInitialProjections,
  CrivSourceEntry,
  CrivStateSummary,
} from "./wasm";
import type { CrivLikeC4Model } from "@criv/likec4/protocol";

export type { CrivArtifactEntry } from "./wasm";
export type { CrivAssetEntry } from "./wasm";

export interface CrivStateSnapshot {
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  assets: CrivAssetEntry[];
  graphNodes: CrivGraphNode[];
  registeredPatterns: string[];
  c4Artifacts: CrivArtifactEntry[];
  architecture?: CrivLikeC4Model;
}

export function buildStateSnapshot(projections: CrivInitialProjections): CrivStateSnapshot {
  return {
    summary: projections.summary,
    sources: projections.sources,
    assets: projections.assets,
    graphNodes: projections.nodes,
    registeredPatterns: projections.registeredPatterns,
    c4Artifacts: projections.c4Artifacts,
    architecture: projections.architecture,
  };
}

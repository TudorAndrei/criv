import type { CrivGraphNode, CrivSourceEntry, CrivStateSummary, CrivValidatedState } from "./wasm";
import type { CrivLikeC4Model } from "@criv/likec4/protocol";

export type CrivStateEnvelope = CrivValidatedState;

export interface CrivStateSnapshot {
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  graphNodes: CrivGraphNode[];
  registeredPatterns: string[];
  c4Artifacts: CrivArtifactEntry[];
  architecture?: CrivLikeC4Model;
}

export interface CrivArtifactEntry {
  path: string;
  label: string;
  target: string;
}

export function registeredPatterns(envelope: CrivStateEnvelope): string[] {
  const explicit = envelope["registered-patterns"];
  if (Array.isArray(explicit)) {
    return explicit.filter((value): value is string => typeof value === "string").sort();
  }

  if (isRecord(envelope.patterns)) {
    return Object.keys(envelope.patterns).sort();
  }

  return [];
}

export function buildStateSnapshot(
  envelope: CrivStateEnvelope,
  summary: CrivStateSummary,
  sources: CrivSourceEntry[],
  graphNodes: CrivGraphNode[],
): CrivStateSnapshot {
  return {
    summary,
    sources,
    graphNodes,
    registeredPatterns: registeredPatterns(envelope),
    c4Artifacts: c4Artifacts(sources, graphNodes),
    architecture: architectureModel(envelope),
  };
}

function architectureModel(envelope: CrivStateEnvelope): CrivLikeC4Model | undefined {
  const architecture = envelope.architecture;
  if (!isRecord(architecture) || !isRecord(architecture.model)) {
    return undefined;
  }
  const model = architecture.model;
  if (!Array.isArray(model.views) || !Array.isArray(model.sourceLinks) || !("raw" in model)) {
    return undefined;
  }
  return {
    protocolVersion: 1,
    likec4Version: String(architecture.likec4Version ?? ""),
    revision: Number(architecture.revision ?? 0),
    workspace: String(architecture.workspace ?? ""),
    model: model.raw,
    views: model.views as { id: string; title: string; sourcePath?: string }[],
    sourceLinks: model.sourceLinks as { element: string; target: string }[],
  } as CrivLikeC4Model;
}

export function c4Artifacts(
  sources: readonly CrivSourceEntry[],
  graphNodes: readonly CrivGraphNode[],
): CrivArtifactEntry[] {
  const artifacts = new Map<string, CrivArtifactEntry>();

  for (const source of sources) {
    if (!isC4Path(source.path)) {
      continue;
    }
    artifacts.set(source.path, {
      path: source.path,
      label: source.path,
      target: source.path,
    });
  }

  for (const node of graphNodes) {
    const target = node.source_target ?? node.path;
    const path = node.path ?? target;
    if (!target || !path || !isC4Path(path)) {
      continue;
    }
    artifacts.set(path, {
      path,
      label: node.label || path,
      target,
    });
  }

  return [...artifacts.values()].sort((left, right) => left.path.localeCompare(right.path));
}

function isC4Path(path: string): boolean {
  return path.split("#", 1)[0]?.endsWith(".c4") ?? false;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

import type { CrivGraphNode, CrivSourceEntry, CrivStateSummary } from "./wasm";

export const CRIV_STATE_SCHEMA = "criv.state.v0";

export interface CrivStateEnvelope {
  schema?: unknown;
  graph?: {
    nodes?: unknown[];
    edges?: unknown[];
  };
  "registered-patterns"?: unknown[];
  "source-index"?: unknown[];
  patterns?: Record<string, unknown[]>;
}

export interface CrivStateSnapshot {
  raw: string;
  summary: CrivStateSummary;
  sources: CrivSourceEntry[];
  graphNodes: CrivGraphNode[];
  registeredPatterns: string[];
  c4Artifacts: CrivArtifactEntry[];
}

export interface CrivArtifactEntry {
  path: string;
  label: string;
  target: string;
}

export type ParsedStateEnvelope =
  | { ok: true; envelope: CrivStateEnvelope }
  | { ok: false; error: string };

export function parseStateEnvelope(raw: string): ParsedStateEnvelope {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) };
  }

  if (!isRecord(parsed)) {
    return { ok: false, error: "State JSON must be an object." };
  }

  if (parsed.schema !== CRIV_STATE_SCHEMA) {
    return {
      ok: false,
      error: `Unsupported criv state schema: ${stringValue(parsed.schema) || "<missing>"}.`,
    };
  }

  return { ok: true, envelope: parsed as CrivStateEnvelope };
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
  raw: string,
  envelope: CrivStateEnvelope,
  summary: CrivStateSummary,
  sources: CrivSourceEntry[],
  graphNodes: CrivGraphNode[],
): CrivStateSnapshot {
  return {
    raw,
    summary,
    sources,
    graphNodes,
    registeredPatterns: registeredPatterns(envelope),
    c4Artifacts: c4Artifacts(sources, graphNodes),
  };
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

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

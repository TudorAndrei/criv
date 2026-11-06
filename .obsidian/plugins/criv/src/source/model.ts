import {
  isRecord,
  parseLineFragment,
  patternTargetId,
  resolveSourceTarget,
  sourceTargetParts,
  type CrivGraphNode,
  type CrivInitialProjections,
  type CrivPatternMatch,
  type CrivSourceEntry,
  type CrivSourceTargetCandidate,
  type SourceLines,
} from "@criv/editor-state";

import type { SourceResolver } from "../ports";

export interface LinkedSource {
  target: string;
  canonicalTarget: string;
  lines?: SourceLines;
  entry: CrivSourceEntry;
  node: CrivGraphNode;
}

export type SourceResolution =
  | { kind: "resolved"; source: LinkedSource }
  | { kind: "unresolved" }
  | {
      kind: "ambiguous";
      candidates: CrivSourceTargetCandidate[];
      totalCandidateCount: number;
    };

export interface FrontmatterPatternTarget {
  id: string;
  source: "targets" | "policy";
  status: "resolved" | "local" | "unresolved";
  matches: CrivPatternMatch[];
}

export interface CrivLinkRange {
  from: number;
  to: number;
  target: string;
  kind: "source" | "pattern" | "unknown";
  status: "resolved" | "unresolved" | "ambiguous";
  candidates?: CrivSourceTargetCandidate[];
  totalCandidateCount?: number;
}

const WIKILINK = /\[\[([^\]]+)\]\]/g;

export function linkedSourcesFromMarkdown(
  markdown: string,
  resolver: SourceResolver,
): LinkedSource[] {
  const sources = new Map<string, LinkedSource>();
  for (const match of markdown.matchAll(WIKILINK)) {
    const source = resolveSource(resolver, match[1] ?? "");
    if (source && !sources.has(source.entry.path)) {
      sources.set(source.entry.path, source);
    }
  }
  return [...sources.values()];
}

export function resolveSource(resolver: SourceResolver, target: string): LinkedSource | null {
  const result = resolveSourceResult(resolver, target);
  return result.kind === "resolved" ? result.source : null;
}

/** Resolve a link to a source file that the loaded State indexes. */
export function resolveSourceResult(resolver: SourceResolver, target: string): SourceResolution {
  const result = resolveSourceTarget((lookup) => resolver.lookupSourceTarget(lookup), target);
  if (result.kind === "ambiguous") {
    return {
      kind: "ambiguous",
      candidates: result.candidates,
      totalCandidateCount: result.total_candidate_count,
    };
  }
  const entry =
    result.kind === "resolved"
      ? resolver.sourceEntry(sourceTargetParts(result.canonical_target).path)
      : undefined;
  if (result.kind !== "resolved" || !entry) {
    return { kind: "unresolved" };
  }
  return {
    kind: "resolved",
    source: {
      target,
      canonicalTarget: result.canonical_target,
      lines: result.lines ?? parseLineFragment(result.node.line_range),
      entry,
      node: result.node,
    },
  };
}

export function resolvePattern(state: CrivInitialProjections, target: string): string | null {
  const id = patternTargetId(target);
  return id !== undefined && state.registeredPatterns.includes(id) ? id : null;
}

export function sourceTooltip(source: LinkedSource): string {
  return `${source.node.kind}: ${source.node.label}`;
}

export function patternTooltip(state: CrivInitialProjections, id: string): string {
  const count = state.patternMatches[id]?.length ?? 0;
  return `${id}: ${count} match${count === 1 ? "" : "es"}`;
}

export function frontmatterPatternTargets(
  frontmatter: Record<string, unknown> | undefined,
  state: CrivInitialProjections,
): FrontmatterPatternTarget[] {
  const noteId = stringValue(frontmatter?.id);
  const fromSection = (section: unknown, source: FrontmatterPatternTarget["source"]) =>
    patternList(isRecord(section) ? section.patterns : undefined).flatMap((pattern) => {
      const target = frontmatterPatternTarget(pattern, source, noteId, state);
      return target ? [target] : [];
    });
  return [
    ...fromSection(frontmatter?.targets, "targets"),
    ...fromSection(frontmatter?.policy, "policy"),
  ];
}

export function crivLinkRanges(
  text: string,
  state: CrivInitialProjections,
  resolver: SourceResolver,
): CrivLinkRange[] {
  const ranges: CrivLinkRange[] = [];
  for (const match of text.matchAll(WIKILINK)) {
    const target = match[1] ?? "";
    const from = match.index;
    const to = from + match[0].length;
    const source = resolveSourceResult(resolver, target);
    if (source.kind === "resolved") {
      ranges.push({ from, to, target, kind: "source", status: "resolved" });
    } else if (source.kind === "ambiguous") {
      ranges.push({
        from,
        to,
        target,
        kind: "source",
        status: "ambiguous",
        candidates: source.candidates,
        totalCandidateCount: source.totalCandidateCount,
      });
    } else if (resolvePattern(state, target)) {
      ranges.push({ from, to, target, kind: "pattern", status: "resolved" });
    } else if (looksLikeSourceOrPattern(target)) {
      ranges.push({ from, to, target, kind: "unknown", status: "unresolved" });
    }
  }
  return ranges;
}

export function looksLikeSourceOrPattern(target: string): boolean {
  return (
    patternTargetId(target) !== undefined ||
    /\.[a-z0-9]+(#.*)?$/i.test((target.split("|", 1)[0] ?? "").trim())
  );
}

function frontmatterPatternTarget(
  pattern: unknown,
  source: FrontmatterPatternTarget["source"],
  noteId: string | null,
  state: CrivInitialProjections,
): FrontmatterPatternTarget | null {
  const rawRef = isRecord(pattern) ? stringValue(pattern.ref) : null;
  const rawId = isRecord(pattern) ? stringValue(pattern.id) : stringValue(pattern);
  const id = rawRef ?? (source === "policy" && rawId && noteId ? `${noteId}/${rawId}` : rawId);
  if (!id) {
    return null;
  }
  if (source === "targets" && !rawRef) {
    return { id, source, status: "local", matches: [] };
  }
  const matches = state.patternMatches[id];
  return {
    id,
    source,
    status: state.registeredPatterns.includes(id) || matches ? "resolved" : "unresolved",
    matches: matches ?? [],
  };
}

function patternList(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value;
  }
  return value ? [value] : [];
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

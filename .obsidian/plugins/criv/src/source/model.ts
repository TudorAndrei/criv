import {
  isRecord,
  looksLikeSourceTarget,
  patternTargetId,
  resolvedSourceLocation,
  resolveSourceTarget,
  sourceTargetProblem,
  wikilinks,
  type CrivGraphNode,
  type CrivInitialProjections,
  type CrivPatternMatch,
  type CrivSourceEntry,
  type SourceLines,
  type SourceTargetResolution,
} from "@criv/editor-state";

import type { SourceResolver } from "../ports";

export interface LinkedSource {
  entry: CrivSourceEntry;
  node: CrivGraphNode;
  lines?: SourceLines;
}

export type SourceResolution =
  | { kind: "resolved"; source: LinkedSource }
  | Exclude<SourceTargetResolution, { kind: "resolved" }>;

/** What a link target means: an indexed source, a registered pattern, or a warning. */
export type CrivLinkStatus =
  | { kind: "source"; source: LinkedSource }
  | { kind: "pattern"; id: string }
  | { kind: "problem"; message: string };

export interface CrivLinkRange {
  from: number;
  to: number;
  target: string;
  status: CrivLinkStatus;
}

export interface FrontmatterPatternTarget {
  id: string;
  source: "targets" | "policy";
  status: "resolved" | "local" | "unresolved";
  matches: CrivPatternMatch[];
}

export function linkedSourcesFromMarkdown(
  markdown: string,
  resolver: SourceResolver,
): LinkedSource[] {
  const sources = new Map<string, LinkedSource>();
  for (const link of wikilinks(markdown)) {
    const source = resolveSource(resolver, link.target);
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
  if (result.kind !== "resolved") {
    return result;
  }
  const location = resolvedSourceLocation(result);
  const entry = location && resolver.sourceEntry(location.path);
  return location && entry
    ? { kind: "resolved", source: { entry, node: result.node, lines: location.lines } }
    : { kind: "unresolved" };
}

export function resolvePattern(state: CrivInitialProjections, target: string): string | null {
  const id = patternTargetId(target);
  return id !== undefined && state.registeredPatterns.includes(id) ? id : null;
}

/**
 * The status of a link from its candidate targets: the first source match,
 * then the first registered pattern, then a warning when the first target
 * names a file or pattern. Null for a link criv does not own, such as a note.
 */
export function crivLinkStatus(
  state: CrivInitialProjections,
  resolver: SourceResolver,
  targets: readonly string[],
): CrivLinkStatus | null {
  for (const target of targets) {
    const result = resolveSourceResult(resolver, target);
    if (result.kind === "resolved") {
      return { kind: "source", source: result.source };
    }
    if (result.kind === "ambiguous") {
      return { kind: "problem", message: sourceTargetProblem(target, result) };
    }
  }
  for (const target of targets) {
    const id = resolvePattern(state, target);
    if (id) {
      return { kind: "pattern", id };
    }
  }
  const [first] = targets;
  return first && (looksLikeSourceTarget(first) || patternTargetId(first) !== undefined)
    ? { kind: "problem", message: sourceTargetProblem(first, { kind: "unresolved" }) }
    : null;
}

export function crivLinkRanges(
  text: string,
  state: CrivInitialProjections,
  resolver: SourceResolver,
): CrivLinkRange[] {
  return wikilinks(text).flatMap((link) => {
    const status = crivLinkStatus(state, resolver, [link.target]);
    return status ? [{ from: link.start, to: link.end, target: link.target, status }] : [];
  });
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

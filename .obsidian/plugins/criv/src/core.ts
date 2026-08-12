export interface CrivNode {
  id: string;
  kind: string;
  label: string;
  path?: string;
  source_target?: string;
  line_range?: string;
}

export interface PatternMatch {
  file: string;
  range?: string;
  captures: Record<string, string>;
}

export interface SourceIndexEntry {
  path: string;
  frecency: number;
  mime?: string;
}

export interface LinkedSource {
  target: string;
  canonicalTarget: string;
  fragment: string | null;
  entry: SourceIndexEntry;
  node: CrivNode;
}

export interface CrivSourceTargetCandidate {
  canonical_target: string;
  node_id: string;
  kind: string;
  label: string;
}

export type CrivSourceTargetLookupResult =
  | { kind: "resolved"; canonical_target: string; node: CrivNode }
  | { kind: "unresolved" }
  | {
      kind: "ambiguous";
      candidates: CrivSourceTargetCandidate[];
      total_candidate_count: number;
    };

export type SourceResolution =
  | { kind: "resolved"; source: LinkedSource }
  | { kind: "unresolved" }
  | { kind: "malformed" }
  | {
      kind: "ambiguous";
      candidates: CrivSourceTargetCandidate[];
      totalCandidateCount: number;
    };

export interface SourceResolver {
  lookupSourceTarget(target: string): CrivSourceTargetLookupResult;
  sourceEntry(path: string): SourceIndexEntry | undefined;
}

export interface CrivState {
  schema: string;
  architecture?: {
    protocolVersion: number;
    likec4Version: string;
    revision: number;
    workspace: string;
    model: {
      raw: unknown;
      views: { id: string; title: string; sourcePath?: string }[];
      sourceLinks: { element: string; target: string }[];
    };
  };
  graph?: { nodes?: CrivNode[] };
  patterns?: Record<string, PatternMatch[]>;
  "registered-patterns"?: string[];
  "source-index"?: SourceIndexEntry[];
}

export interface FrontmatterPatternTarget {
  id: string;
  source: "targets" | "policy";
  status: "resolved" | "local" | "unresolved";
  matches: PatternMatch[];
}

export interface CrivLinkRange {
  from: number;
  to: number;
  target: string;
  kind: "source" | "pattern" | "unknown";
  status: "resolved" | "unresolved" | "ambiguous" | "malformed";
  candidates?: CrivSourceTargetCandidate[];
  totalCandidateCount?: number;
}

export function linkedSourcesFromMarkdown(
  markdown: string,
  resolver: SourceResolver,
): LinkedSource[] {
  const links = Array.from(markdown.matchAll(/\[\[([^\]]+)\]\]/g))
    .map((match) => match[1] ?? "")
    .map((target) => resolveSource(resolver, target))
    .filter((source): source is LinkedSource => source !== null);
  const seen = new Set<string>();
  return links.filter((source) => {
    if (seen.has(source.entry.path)) {
      return false;
    }
    seen.add(source.entry.path);
    return true;
  });
}

export function safeVaultPath(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const path = value.trim().replace(/\\/g, "/");
  if (
    !path ||
    path.startsWith("/") ||
    path.startsWith("//") ||
    /^[A-Za-z]:/.test(path) ||
    path.includes("\0")
  ) {
    return null;
  }
  const segments = path.split("/");
  const normalized: string[] = [];
  for (const segment of segments) {
    if (!segment || segment === ".") {
      continue;
    }
    if (segment === "..") {
      return null;
    }
    normalized.push(segment);
  }
  return normalized.length > 0 ? normalized.join("/") : null;
}

export function parseLineRange(fragment: string | null): { start: number; end: number } | null {
  const match = fragment?.match(/^L(\d+)(?:-L?(\d+))?$/i);
  if (!match) {
    return null;
  }
  const start = Number(match[1]);
  const end = Number(match[2] ?? match[1]);
  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return null;
  }
  return { start: Math.max(1, start), end: Math.max(start, end) };
}

export function addTextTargets(targets: string[], value: string | null | undefined): void {
  if (!value) {
    return;
  }
  addTarget(targets, value);
  for (const match of value.matchAll(/\[\[([^\]]+)\]\]/g)) {
    addTarget(targets, match[1]);
  }
  const stripped = value.replace(/^\[\[/, "").replace(/\]\]$/, "");
  if (stripped !== value) {
    addTarget(targets, stripped);
  }
}

export function addTarget(targets: string[], value: string | null | undefined): void {
  const target = value?.trim();
  if (target) {
    targets.push(target);
  }
}

export function decodeSourceLinkTarget(target: string): string | null {
  try {
    return decodeURIComponent(target);
  } catch {
    return null;
  }
}

export function resolveSource(resolver: SourceResolver, target: string): LinkedSource | null {
  const result = resolveSourceResult(resolver, target);
  return result.kind === "resolved" ? result.source : null;
}

export function resolveSourceResult(resolver: SourceResolver, target: string): SourceResolution {
  const clean = cleanTarget(target);
  const [targetPath, fragment] = clean.split("#", 2);
  if (!targetPath || targetPath.startsWith("match:")) {
    return { kind: "unresolved" };
  }
  const sourcePath = targetPath.startsWith("source:")
    ? targetPath.slice("source:".length)
    : targetPath;
  if (!sourcePath) {
    return { kind: "malformed" };
  }
  let lookupTarget = sourcePath;
  if (fragment) {
    if (parseLineRange(fragment)) {
      lookupTarget = sourcePath;
    } else if (/^l/i.test(fragment)) {
      return { kind: "malformed" };
    } else {
      lookupTarget = `${sourcePath}#${fragment}`;
    }
  }

  const result = resolver.lookupSourceTarget(lookupTarget);
  if (result.kind === "unresolved") {
    return { kind: "unresolved" };
  }
  if (result.kind === "ambiguous") {
    return {
      kind: "ambiguous",
      candidates: result.candidates,
      totalCandidateCount: result.total_candidate_count,
    };
  }
  if (result.kind === "resolved") {
    // Continue with the canonical source below.
  } else {
    return { kind: "unresolved" };
  }

  const canonicalPath = result.canonical_target.split("#", 1)[0];
  const entry = canonicalPath ? resolver.sourceEntry(canonicalPath) : undefined;
  if (!entry) {
    return { kind: "unresolved" };
  }
  return {
    kind: "resolved",
    source: {
      target,
      canonicalTarget: result.canonical_target,
      fragment: fragment ?? null,
      entry,
      node: result.node,
    },
  };
}

export function resolvePattern(state: CrivState, target: string): string | null {
  const clean = cleanTarget(target);
  const id = clean.startsWith("match:") ? clean.slice("match:".length) : clean.split("#match:")[1];
  if (!id) {
    return null;
  }
  const ids = state["registered-patterns"] ?? [];
  return ids.includes(id) ? id : null;
}

export function sourceTooltip(source: LinkedSource): string {
  return `${source.node.kind}: ${source.node.label}`;
}

export function patternTooltip(state: CrivState, id: string): string {
  const count = state.patterns?.[id]?.length ?? 0;
  return `${id}: ${count} match${count === 1 ? "" : "es"}`;
}

export function frontmatterPatternTargets(
  frontmatter: Record<string, unknown> | undefined,
  state: CrivState,
): FrontmatterPatternTarget[] {
  const targets: FrontmatterPatternTarget[] = [];
  const noteId = stringValue(frontmatter?.id);
  const targetObject = objectValue(frontmatter?.targets);
  for (const pattern of patternList(targetObject?.patterns)) {
    const target = frontmatterPatternTarget(pattern, "targets", noteId, state);
    if (target) {
      targets.push(target);
    }
  }

  const policyObject = objectValue(frontmatter?.policy);
  for (const pattern of patternList(policyObject?.patterns)) {
    const target = frontmatterPatternTarget(pattern, "policy", noteId, state);
    if (target) {
      targets.push(target);
    }
  }
  return targets;
}

export function crivLinkRanges(
  text: string,
  state: CrivState,
  resolver: SourceResolver,
): CrivLinkRange[] {
  const ranges: CrivLinkRange[] = [];
  for (const match of text.matchAll(/\[\[([^\]]+)\]\]/g)) {
    const rawTarget = match[1] ?? "";
    const from = match.index ?? 0;
    const to = from + match[0].length;
    const source = resolveSourceResult(resolver, rawTarget);
    if (source.kind === "resolved") {
      ranges.push({ from, to, target: rawTarget, kind: "source", status: "resolved" });
      continue;
    }
    if (source.kind === "ambiguous" || source.kind === "malformed") {
      ranges.push({
        from,
        to,
        target: rawTarget,
        kind: "source",
        status: source.kind,
        candidates: source.kind === "ambiguous" ? source.candidates : undefined,
        totalCandidateCount: source.kind === "ambiguous" ? source.totalCandidateCount : undefined,
      });
      continue;
    }
    const pattern = resolvePattern(state, rawTarget);
    if (pattern) {
      ranges.push({ from, to, target: rawTarget, kind: "pattern", status: "resolved" });
      continue;
    }
    if (looksLikeSourceOrPattern(rawTarget)) {
      ranges.push({ from, to, target: rawTarget, kind: "unknown", status: "unresolved" });
    }
  }
  return ranges;
}

export function looksLikeSourceOrPattern(target: string): boolean {
  const clean = cleanTarget(target);
  return clean.startsWith("match:") || /\.[a-z0-9]+(#.*)?$/i.test(clean);
}

export function cleanTarget(target: string): string {
  return target.split("|")[0]?.trim() ?? "";
}

function frontmatterPatternTarget(
  pattern: unknown,
  source: FrontmatterPatternTarget["source"],
  noteId: string | null,
  state: CrivState,
): FrontmatterPatternTarget | null {
  const object = objectValue(pattern);
  const rawRef = object ? stringValue(object.ref) : null;
  const rawId = object ? stringValue(object.id) : stringValue(pattern);
  const id = rawRef ?? (source === "policy" && rawId && noteId ? `${noteId}/${rawId}` : rawId);
  if (!id) {
    return null;
  }
  if (source === "targets" && !rawRef) {
    return { id, source, status: "local", matches: [] };
  }

  const matches = state.patterns?.[id] ?? [];
  const ids = state["registered-patterns"] ?? [];
  return {
    id,
    source,
    status: ids.includes(id) || state.patterns?.[id] ? "resolved" : "unresolved",
    matches,
  };
}

function patternList(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value;
  }
  return value ? [value] : [];
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

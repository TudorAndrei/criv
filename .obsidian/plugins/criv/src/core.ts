export interface CrivNode {
  id: string;
  kind: string;
  label: string;
  path?: string;
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
  fragment: string | null;
  entry: SourceIndexEntry;
}

export interface LinkedNote {
  target: string;
  fragment: string | null;
  node: CrivNode;
  path: string;
}

export interface CrivState {
  schema: string;
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
  kind: "source" | "pattern" | "note" | "unknown";
  status: "resolved" | "unresolved";
}

interface ScoredSource {
  entry: SourceIndexEntry;
  score: number;
}

export function linkedSourcesFromMarkdown(markdown: string, state: CrivState): LinkedSource[] {
  const links = Array.from(markdown.matchAll(/\[\[([^\]]+)\]\]/g))
    .map((match) => match[1] ?? "")
    .map((target) => resolveSource(state, target))
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

export function sourceEntries(state: CrivState | null | undefined): SourceIndexEntry[] {
  const entries = state?.["source-index"] ?? [];
  const seen = new Set<string>();
  return entries.filter((entry) => {
    if (!entry.path || seen.has(entry.path)) {
      return false;
    }
    seen.add(entry.path);
    return true;
  });
}

export function sourceSuggestions(
  state: CrivState | null | undefined,
  query: string,
  limit = 20,
): SourceIndexEntry[] {
  const cleanQuery = query.trim().toLowerCase();
  const entries = sourceEntries(state);
  if (!cleanQuery) {
    return entries
      .slice()
      .sort((left, right) => right.frecency - left.frecency || left.path.localeCompare(right.path))
      .slice(0, limit);
  }

  const scored = entries
    .map((entry): ScoredSource | null => {
      const score = sourceMatchScore(entry.path, cleanQuery);
      return score === null ? null : { entry, score: score + entry.frecency };
    })
    .filter((row): row is ScoredSource => row !== null);

  return scored
    .sort(
      (left, right) =>
        right.score - left.score ||
        right.entry.frecency - left.entry.frecency ||
        left.entry.path.localeCompare(right.entry.path),
    )
    .map((row) => row.entry)
    .slice(0, limit);
}

export function resolveSource(state: CrivState, target: string): LinkedSource | null {
  const clean = cleanTarget(target);
  const normalized = clean.split("#")[0] ?? "";
  if (!normalized || normalized.startsWith("match:")) {
    return null;
  }
  const entry = resolveSourceEntry(state, normalized);
  if (!entry) {
    return null;
  }
  return {
    target,
    fragment: clean.includes("#") ? clean.split("#").slice(1).join("#") : null,
    entry,
  };
}

export function resolveNote(state: CrivState, target: string): LinkedNote | null {
  const clean = cleanTarget(target);
  const normalized = clean.split("#")[0]?.trim() ?? "";
  if (!normalized || normalized.startsWith("match:") || clean.includes("#match:")) {
    return null;
  }
  const node = resolveNoteNode(state, normalized);
  if (!node?.path) {
    return null;
  }
  return {
    target,
    fragment: clean.includes("#") ? clean.split("#").slice(1).join("#") : null,
    node,
    path: node.path,
  };
}

export function resolvePattern(state: CrivState, target: string): string | null {
  const clean = cleanTarget(target);
  const id = clean.startsWith("match:") ? clean.slice("match:".length) : clean.split("#match:")[1];
  if (!id) {
    return null;
  }
  const ids = state["registered-patterns"] ?? Object.keys(state.patterns ?? {});
  return ids.includes(id) ? id : null;
}

export function sourceTooltip(state: CrivState, source: SourceIndexEntry): string {
  const node = state.graph?.nodes?.find((candidate) => candidate.path === source.path);
  return node ? `${node.kind}: ${node.label}` : source.path;
}

export function noteTooltip(note: LinkedNote): string {
  return `${note.node.kind}: ${note.node.label}`;
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

export function crivLinkRanges(text: string, state: CrivState): CrivLinkRange[] {
  const ranges: CrivLinkRange[] = [];
  for (const match of text.matchAll(/\[\[([^\]]+)\]\]/g)) {
    const rawTarget = match[1] ?? "";
    const from = match.index ?? 0;
    const to = from + match[0].length;
    const source = resolveSource(state, rawTarget);
    if (source) {
      ranges.push({ from, to, target: rawTarget, kind: "source", status: "resolved" });
      continue;
    }
    const pattern = resolvePattern(state, rawTarget);
    if (pattern) {
      ranges.push({ from, to, target: rawTarget, kind: "pattern", status: "resolved" });
      continue;
    }
    const note = resolveNote(state, rawTarget);
    if (note) {
      ranges.push({ from, to, target: rawTarget, kind: "note", status: "resolved" });
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

function resolveSourceEntry(state: CrivState, targetPath: string): SourceIndexEntry | null {
  const entries = sourceEntries(state);
  return (
    entries.find((candidate) => candidate.path === targetPath) ??
    entries.find(
      (candidate) =>
        candidate.path.endsWith(targetPath) || candidate.path.split("/").pop() === targetPath,
    ) ??
    null
  );
}

function resolveNoteNode(state: CrivState, target: string): CrivNode | null {
  const key = target.toLowerCase();
  const nodes = (state.graph?.nodes ?? []).filter(isNoteNode);
  return (
    nodes.find((candidate) => noteKey(candidate) === key) ??
    nodes.find((candidate) => noteFilenameStem(candidate) === key) ??
    nodes.find((candidate) => candidate.path?.toLowerCase() === key) ??
    nodes.find((candidate) => candidate.label.toLowerCase() === key) ??
    null
  );
}

function noteKey(node: CrivNode): string | null {
  return isNoteNode(node) ? node.id.replace(/^note:/, "").toLowerCase() : null;
}

function noteFilenameStem(node: CrivNode): string | null {
  if (!isNoteNode(node) || !node.path) {
    return null;
  }
  const basename = node.path.split("/").pop() ?? node.path;
  return basename.replace(/\.md$/i, "").toLowerCase();
}

function isNoteNode(node: CrivNode): boolean {
  return (node.kind === "doc" || node.kind === "decision") && node.id.startsWith("note:");
}

function sourceMatchScore(path: string, query: string): number | null {
  const lowerPath = path.toLowerCase();
  const basename = lowerPath.split("/").pop() ?? lowerPath;
  if (lowerPath === query) {
    return 100_000;
  }
  if (basename === query) {
    return 90_000;
  }
  if (lowerPath.endsWith(query)) {
    return 80_000 - lowerPath.length;
  }
  if (basename.startsWith(query)) {
    return 70_000 - basename.length;
  }
  if (lowerPath.includes(query)) {
    return 60_000 - lowerPath.indexOf(query) - lowerPath.length;
  }
  const fuzzyScore = fuzzySubsequenceScore(lowerPath, query);
  return fuzzyScore === null ? null : 40_000 + fuzzyScore - lowerPath.length;
}

function fuzzySubsequenceScore(value: string, query: string): number | null {
  let queryIndex = 0;
  let score = 0;
  let run = 0;
  for (let index = 0; index < value.length && queryIndex < query.length; index += 1) {
    if (value[index] !== query[queryIndex]) {
      run = 0;
      continue;
    }
    run += 1;
    score += run * 3 + (index === 0 || value[index - 1] === "/" ? 8 : 0);
    queryIndex += 1;
  }
  return queryIndex === query.length ? score : null;
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
  const ids = state["registered-patterns"] ?? Object.keys(state.patterns ?? {});
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

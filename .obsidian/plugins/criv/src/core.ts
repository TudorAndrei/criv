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

export interface CrivState {
  schema: string;
  architecture?: {
    protocolVersion: number;
    likec4Version: string;
    revision: number;
    workspace: string;
    model: {
      raw: unknown;
      views: { id: string; title: string }[];
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
  status: "resolved" | "unresolved";
}

export type C4ArtifactFormat = "likec4" | "unknown";
export type C4ArtifactLevel = "model";

export interface C4ArtifactDiagnostic {
  code: string;
  line: number | null;
  message: string;
}

export interface C4ArtifactSummary {
  format: C4ArtifactFormat;
  level: C4ArtifactLevel;
  generated: boolean;
  diagnostics: C4ArtifactDiagnostic[];
}

export function linkedSourcesFromMarkdown(
  markdown: string,
  sources: readonly SourceIndexEntry[],
): LinkedSource[] {
  const links = Array.from(markdown.matchAll(/\[\[([^\]]+)\]\]/g))
    .map((match) => match[1] ?? "")
    .map((target) => resolveSource(sources, target))
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

export function resolveSource(
  sources: readonly SourceIndexEntry[],
  target: string,
): LinkedSource | null {
  const clean = cleanTarget(target);
  const normalized = clean.split("#")[0] ?? "";
  if (!normalized || normalized.startsWith("match:")) {
    return null;
  }
  const entry = resolveSourceEntry(sources, normalized);
  if (!entry) {
    return null;
  }
  return {
    target,
    fragment: clean.includes("#") ? clean.split("#").slice(1).join("#") : null,
    entry,
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
  sources: readonly SourceIndexEntry[],
): CrivLinkRange[] {
  const ranges: CrivLinkRange[] = [];
  for (const match of text.matchAll(/\[\[([^\]]+)\]\]/g)) {
    const rawTarget = match[1] ?? "";
    const from = match.index ?? 0;
    const to = from + match[0].length;
    const source = resolveSource(sources, rawTarget);
    if (source) {
      ranges.push({ from, to, target: rawTarget, kind: "source", status: "resolved" });
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

export function parseC4Artifact(_path: string, text: string): C4ArtifactSummary {
  const generated = /^\s*\/\/\s*criv:generated\s+true\s*$/m.test(text);
  const format = /\b(specification|model|views|deployment|global|extend)\s*\{/.test(text)
    ? "likec4"
    : "unknown";
  return {
    format,
    level: "model",
    generated,
    diagnostics:
      format === "likec4"
        ? []
        : [
            {
              code: "unknown-c4-format",
              line: firstNonEmptyLine(text),
              message: "The .c4 file must contain LikeC4 DSL.",
            },
          ],
  };
}

export function looksLikeSourceOrPattern(target: string): boolean {
  const clean = cleanTarget(target);
  return clean.startsWith("match:") || /\.[a-z0-9]+(#.*)?$/i.test(clean);
}

export function cleanTarget(target: string): string {
  return target.split("|")[0]?.trim() ?? "";
}

function firstNonEmptyLine(text: string): number | null {
  const lines = text.split(/\r?\n/);
  const index = lines.findIndex((line) => line.trim().length > 0);
  return index === -1 ? null : index + 1;
}

function resolveSourceEntry(
  entries: readonly SourceIndexEntry[],
  targetPath: string,
): SourceIndexEntry | null {
  return (
    entries.find((candidate) => candidate.path === targetPath) ??
    entries.find(
      (candidate) =>
        candidate.path.endsWith(targetPath) || candidate.path.split("/").pop() === targetPath,
    ) ??
    null
  );
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

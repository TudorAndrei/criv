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

export type StateInterpretation =
  | { state: CrivState }
  | { error: string; kind: "parse" | "schema" };

export type C4ArtifactFormat = "mermaid" | "dot" | "unknown";
export type C4ArtifactLevel = "context" | "container" | "component" | "code" | "unknown";

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
  const safeEntries: SourceIndexEntry[] = [];
  for (const entry of entries) {
    const path = safeVaultPath(entry.path);
    if (!path || seen.has(path)) {
      continue;
    }
    seen.add(path);
    safeEntries.push({ ...entry, path });
  }
  return safeEntries;
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

export function interpretState(raw: string, expectedSchema: string): StateInterpretation {
  let state: CrivState;
  try {
    state = JSON.parse(raw) as CrivState;
  } catch (error) {
    return { error: errorMessage(error), kind: "parse" };
  }

  if (state.schema !== expectedSchema) {
    return {
      error: `Unsupported criv state schema ${state.schema ?? "unknown"}`,
      kind: "schema",
    };
  }
  return { state };
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

export function renderErrorsMessage(errors: { level?: string; message: string }[]): string {
  return errors.map((error) => error.message).join("; ") || "Graphviz render failed";
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

/**
 * Fallback source-suggestion ranking for Obsidian when criv-wasm is unavailable.
 * Keep this scorer in sync with the wasm port; the parity test covers ASCII paths.
 */
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

export function sanitizeDotSvg(svg: string): string {
  return svg
    .replace(/<\?xml[\s\S]*?\?>/gi, "")
    .replace(/<!DOCTYPE[\s\S]*?>/gi, "")
    .replace(
      /<\s*(script|foreignObject|iframe|object|embed|image|use)\b[\s\S]*?<\s*\/\s*\1\s*>/gi,
      "",
    )
    .replace(/<\s*(script|foreignObject|iframe|object|embed|image|use)\b[^>]*\/?>/gi, "")
    .replace(/\s+on[a-z0-9_-]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, "")
    .replace(/\s+(?:href|xlink:href|target)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, "");
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
    if (looksLikeSourceOrPattern(rawTarget)) {
      ranges.push({ from, to, target: rawTarget, kind: "unknown", status: "unresolved" });
    }
  }
  return ranges;
}

export function parseC4Artifact(path: string, text: string): C4ArtifactSummary {
  const diagnostics: C4ArtifactDiagnostic[] = [];
  const level = c4LevelFromPath(path);
  if (level === "unknown") {
    diagnostics.push({
      code: "missing-c4-level",
      line: null,
      message: "Filename should include context, container, component, components, or code.",
    });
  }

  const directives = c4Directives(text);
  let directiveFormat: C4ArtifactFormat = "unknown";
  let assertedFormat: { key: string; value: string | null; line: number } | null = null;
  const generatedDirective = directives.find((directive) => directive.key === "generated");
  for (const directive of directives) {
    if (directive.key === "format") {
      const format = c4FormatFromDirective(directive.value);
      if (format === "unknown") {
        diagnostics.push({
          code: "invalid-c4-format",
          line: directive.line,
          message: "criv:format should be mermaid or dot.",
        });
      } else {
        if (directiveFormat !== "unknown" && directiveFormat !== format) {
          diagnostics.push({
            code: "duplicate-c4-format",
            line: directive.line,
            message: "Conflicting criv:format directives.",
          });
        }
        directiveFormat = format;
        assertedFormat = directive;
      }
    }
    if (!["format", "generated", "source"].includes(directive.key)) {
      diagnostics.push({
        code: "unknown-c4-directive",
        line: directive.line,
        message: `Unknown directive criv:${directive.key}.`,
      });
    }
  }

  const inferredFormat = c4FormatFromText(text);
  if (
    assertedFormat &&
    directiveFormat !== "unknown" &&
    inferredFormat !== "unknown" &&
    directiveFormat !== inferredFormat
  ) {
    diagnostics.push({
      code: "mismatched-c4-format",
      line: firstMeaningfulLine(text)?.line ?? assertedFormat.line,
      message: `criv:format ${directiveFormat} does not match ${inferredFormat} content.`,
    });
  }
  if (inferredFormat === "unknown" && directiveFormat === "unknown") {
    diagnostics.push({
      code: "unknown-c4-format",
      line: firstNonEmptyLine(text),
      message: "Content should start with Mermaid C4 or DOT syntax.",
    });
  }

  const format = inferredFormat !== "unknown" ? inferredFormat : directiveFormat;
  const header = firstMeaningfulLine(text)?.text ?? "";
  if (format === "mermaid") {
    const headerLevel = c4LevelFromMermaidHeader(header);
    if (headerLevel === "unknown") {
      diagnostics.push({
        code: "invalid-c4-mermaid",
        line: firstMeaningfulLine(text)?.line ?? null,
        message: "Mermaid .c4 content should start with C4Context, C4Container, or C4Component.",
      });
    } else if (level === "code" || (level !== "unknown" && level !== headerLevel)) {
      diagnostics.push({
        code: "mismatched-c4-level",
        line: firstMeaningfulLine(text)?.line ?? null,
        message: `Filename level ${level} does not match Mermaid header ${headerLevel}.`,
      });
    }
  }
  if (format === "dot" && level !== "unknown" && level !== "code") {
    diagnostics.push({
      code: "invalid-c4-level",
      line: null,
      message: "DOT .c4 artifacts are expected to be code-level files.",
    });
  }
  if (
    generatedDirective?.value &&
    generatedDirective.value !== "true" &&
    generatedDirective.value !== "false"
  ) {
    diagnostics.push({
      code: "invalid-c4-generated",
      line: generatedDirective.line,
      message: "criv:generated should be true or false.",
    });
  }

  return {
    format,
    level,
    generated: generatedDirective?.value === "true",
    diagnostics,
  };
}

export function looksLikeSourceOrPattern(target: string): boolean {
  const clean = cleanTarget(target);
  return clean.startsWith("match:") || /\.[a-z0-9]+(#.*)?$/i.test(clean);
}

export function cleanTarget(target: string): string {
  return target.split("|")[0]?.trim() ?? "";
}

function c4Directives(text: string): { key: string; value: string | null; line: number }[] {
  return text
    .split(/\r?\n/)
    .map((line, index) => ({ line: index + 1, text: c4CommentText(line.trim()) }))
    .filter((line): line is { line: number; text: string } => line.text !== null)
    .map((line) => ({ line: line.line, text: line.text.trim() }))
    .filter((line) => line.text.startsWith("criv:"))
    .map((line) => {
      const body = line.text.slice("criv:".length).trim();
      const match = body.match(/^(\S+)(?:\s+(.+))?$/);
      return {
        key: match?.[1] ?? "",
        value: match?.[2]?.trim() ?? null,
        line: line.line,
      };
    })
    .filter((directive) => directive.key.length > 0);
}

function c4CommentText(line: string): string | null {
  if (line.startsWith("%%")) {
    return line.slice(2).trim();
  }
  if (line.startsWith("//")) {
    return line.slice(2).trim();
  }
  if (line.startsWith("#")) {
    return line.slice(1).trim();
  }
  return null;
}

function c4FormatFromText(text: string): C4ArtifactFormat {
  const first = firstMeaningfulLine(text)?.text ?? "";
  if (["C4Context", "C4Container", "C4Component"].includes(first)) {
    return "mermaid";
  }
  if (/^(strict\s+)?(di)?graph(?:\s|\{|$)/.test(first)) {
    return "dot";
  }
  return "unknown";
}

function c4FormatFromDirective(value: string | null): C4ArtifactFormat {
  switch ((value ?? "").trim().toLowerCase()) {
    case "mermaid":
    case "mermaid-c4":
      return "mermaid";
    case "dot":
    case "graphviz":
      return "dot";
    default:
      return "unknown";
  }
}

function c4LevelFromPath(path: string): C4ArtifactLevel {
  const stem = (path.split("/").pop() ?? path).replace(/\.[^.]+$/, "").toLowerCase();
  const tokens = stem.split(/[^a-z0-9]+/).filter(Boolean);
  if (tokens.includes("context")) {
    return "context";
  }
  if (tokens.includes("container") || tokens.includes("containers")) {
    return "container";
  }
  if (tokens.includes("component") || tokens.includes("components")) {
    return "component";
  }
  if (tokens.includes("code")) {
    return "code";
  }
  return "unknown";
}

function c4LevelFromMermaidHeader(header: string): C4ArtifactLevel {
  switch (header) {
    case "C4Context":
      return "context";
    case "C4Container":
      return "container";
    case "C4Component":
      return "component";
    default:
      return "unknown";
  }
}

function firstMeaningfulLine(text: string): { line: number; text: string } | null {
  const lines = text.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]?.trim() ?? "";
    if (line && c4CommentText(line) === null) {
      return { line: index + 1, text: line };
    }
  }
  return null;
}

function firstNonEmptyLine(text: string): number | null {
  const lines = text.split(/\r?\n/);
  const index = lines.findIndex((line) => line.trim().length > 0);
  return index === -1 ? null : index + 1;
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

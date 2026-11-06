import type {
  CrivGraphNode,
  CrivSourceTargetCandidate,
  CrivSourceTargetLookupResult,
} from "./contract.ts";

/** One-based, inclusive line range from a `#L<start>` or `#L<start>-L<end>` fragment. */
export interface SourceLines {
  start: number;
  end: number;
}

export interface SourceTargetParts {
  /** Target text without a `|alias` and without the `source:` prefix. */
  body: string;
  path: string;
  fragment?: string;
  lines?: SourceLines;
}

export type ResolvedSourceTarget = {
  kind: "resolved";
  canonical_target: string;
  node: CrivGraphNode;
  lines?: SourceLines;
};

export type SourceTargetResolution =
  | ResolvedSourceTarget
  | Exclude<CrivSourceTargetLookupResult, { kind: "resolved" }>;

/** The file and lines a resolved source target opens. */
export interface SourceLocation {
  path: string;
  lines?: SourceLines;
}

/** A `[[target|alias]]` link: `target` excludes the alias; offsets span the brackets. */
export interface Wikilink {
  target: string;
  start: number;
  end: number;
}

const SOURCE_PREFIX = "source:";
const PATTERN_PREFIX = "match:";
const LINE_FRAGMENT = /^L([1-9]\d*)(?:-L([1-9]\d*))?$/;
const WIKILINK = /\[\[([^\]]+)\]\]/g;
const FILE_EXTENSION = /\.[A-Za-z0-9]+$/;

/**
 * The normalized repository-relative form of `value`, or undefined when it is
 * empty, absolute, drive-qualified, contains NUL, or escapes the root. Follows
 * `safe_relative_path` in `crates/criv-wasm/src/path.rs`; both read
 * `fixtures/editor/safe-vault-path.v1.json`.
 */
export function safeVaultPath(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const path = value.trim().replace(/\\/g, "/");
  const driveQualified = path[1] === ":" && path.charCodeAt(0) < 0x80;
  if (!path || path.startsWith("/") || driveQualified || path.includes("\0")) {
    return undefined;
  }
  const segments: string[] = [];
  for (const segment of path.split("/")) {
    if (segment === "..") {
      return undefined;
    }
    if (segment && segment !== ".") {
      segments.push(segment);
    }
  }
  return segments.length > 0 ? segments.join("/") : undefined;
}

export function parseLineFragment(fragment: string | undefined): SourceLines | undefined {
  const match = fragment === undefined ? null : LINE_FRAGMENT.exec(fragment);
  if (!match) {
    return undefined;
  }
  const start = Number(match[1]);
  return { start, end: Math.max(start, Number(match[2] ?? match[1])) };
}

export function sourceTargetParts(target: string): SourceTargetParts {
  const unaliased = (target.split("|", 1)[0] ?? "").trim();
  const body = unaliased.startsWith(SOURCE_PREFIX)
    ? unaliased.slice(SOURCE_PREFIX.length)
    : unaliased;
  const hash = body.indexOf("#");
  if (hash < 0) {
    return { body, path: body };
  }
  const fragment = body.slice(hash + 1);
  return { body, path: body.slice(0, hash), fragment, lines: parseLineFragment(fragment) };
}

/** The pattern id of a `match:<id>` or `<note>#match:<id>` link target. */
export function patternTargetId(target: string): string | undefined {
  const unaliased = (target.split("|", 1)[0] ?? "").trim();
  if (unaliased.startsWith(PATTERN_PREFIX)) {
    return unaliased.slice(PATTERN_PREFIX.length) || undefined;
  }
  const marker = unaliased.indexOf(`#${PATTERN_PREFIX}`);
  return marker < 0 ? undefined : unaliased.slice(marker + PATTERN_PREFIX.length + 1) || undefined;
}

/**
 * Resolve a source link target through the loaded State. A line fragment is
 * display position, not identity: the path is looked up and the lines are kept.
 */
export function resolveSourceTarget(
  lookup: (target: string) => CrivSourceTargetLookupResult,
  target: string,
): SourceTargetResolution {
  if (patternTargetId(target) !== undefined) {
    return { kind: "unresolved" };
  }
  const parts = sourceTargetParts(target);
  if (!parts.path) {
    return { kind: "unresolved" };
  }
  const result = lookup(parts.lines ? parts.path : parts.body);
  return result.kind === "resolved" && parts.lines ? { ...result, lines: parts.lines } : result;
}

/** The node's file and the requested lines, or the node's own lines without a line fragment. */
export function resolvedSourceLocation(resolved: ResolvedSourceTarget): SourceLocation | undefined {
  const path = safeVaultPath(
    sourceTargetParts(resolved.node.path ?? resolved.canonical_target).path,
  );
  if (!path) {
    return undefined;
  }
  return { path, lines: resolved.lines ?? parseLineFragment(resolved.node.line_range) };
}

export function wikilinks(text: string): Wikilink[] {
  return Array.from(text.matchAll(WIKILINK), (match) => ({
    target: (match[1]?.split("|", 1)[0] ?? "").trim(),
    start: match.index,
    end: match.index + match[0].length,
  }));
}

/** Whether an untyped link names a file: its path, before any fragment, has an extension. */
export function looksLikeSourceTarget(target: string): boolean {
  return FILE_EXTENSION.test(sourceTargetParts(target).path);
}

/** The diagnostic code `criv check` gives a Wikilink that resolves to source. */
export const SOURCE_WIKILINK = "source-wikilink";

/**
 * The warning for a Wikilink that resolves to source. Follows the
 * `source-wikilink` rule of `validate_note_links` in `src/check.rs`: Wikilinks
 * are reserved for document references, and code takes an AST-aware selector.
 */
export function sourceWikilinkWarning(resolved: ResolvedSourceTarget): string {
  return `Wikilinks are reserved for document references; use AST-aware source selector ${resolved.canonical_target}.`;
}

/** The editor warning for a target that does not resolve, or undefined when it resolves. */
export function sourceTargetProblem(
  target: string,
  resolution: Exclude<SourceTargetResolution, { kind: "resolved" }>,
): string;
export function sourceTargetProblem(
  target: string,
  resolution: SourceTargetResolution,
): string | undefined;
export function sourceTargetProblem(
  target: string,
  resolution: SourceTargetResolution,
): string | undefined {
  switch (resolution.kind) {
    case "resolved":
      return undefined;
    case "unresolved":
      return `Unresolved criv target: ${target}.`;
    case "ambiguous":
      return ambiguousSourceTargetMessage(
        target,
        resolution.candidates,
        resolution.total_candidate_count,
      );
  }
}

export function ambiguousSourceTargetMessage(
  target: string,
  candidates: readonly CrivSourceTargetCandidate[],
  totalCandidateCount: number,
): string {
  const details = candidates.map((candidate) => {
    const base = `${candidate.canonical_target} (${candidate.kind}: ${candidate.label})`;
    const sharesTarget = candidates.some(
      (other) => other !== candidate && other.canonical_target === candidate.canonical_target,
    );
    return sharesTarget ? `${base} [${candidate.node_id}]` : base;
  });
  const remaining = totalCandidateCount - candidates.length;
  const suffix = remaining > 0 ? `; ${remaining} more` : "";
  return `Ambiguous criv source target ${target}: ${details.join("; ")}${suffix}.`;
}

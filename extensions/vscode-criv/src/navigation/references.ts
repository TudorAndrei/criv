import {
  ambiguousSourceTargetMessage,
  parseLineFragment,
  resolveSourceTarget,
  safeVaultPath,
  sourceTargetParts,
  type CrivGraphNode,
  type CrivSourceTargetCandidate,
  type CrivSourceTargetLookupResult,
  type SourceLines,
  type SourceTargetResolution,
} from "@criv/editor-state";

export type SourceReferenceKind =
  | "selector"
  | "criv-source"
  | "legacy-wikilink"
  | "typed-source-wikilink";

export interface SourceReference {
  kind: SourceReferenceKind;
  target: string;
  canonicalTarget?: string;
  node?: CrivGraphNode;
  resolutionKind: SourceTargetResolution["kind"];
  candidates?: CrivSourceTargetCandidate[];
  totalCandidateCount?: number;
  start: number;
  end: number;
  legacy: boolean;
  shouldDiagnoseUnresolved: boolean;
}

export type SourceTargetLookup = (target: string) => CrivSourceTargetLookupResult;

export type SourceTargetOpenPlan =
  | { kind: "resolved"; path: string; lines?: SourceLines }
  | Exclude<SourceTargetResolution, { kind: "resolved" }>;

export interface MarkdownSink {
  appendMarkdown(value: string): void;
  appendText(value: string): void;
}

const CRIV_SOURCE_DIRECTIVE = /\bcriv:source\s+([^\s"',)]+)/g;
const SOURCE_WIKILINK = /\[\[(source:)?([^\]|\s]+)(?:\|[^\]]*)?\]\]/g;
const SELECTOR_CANDIDATE =
  /(^|[^\w./:#-])([A-Za-z0-9_.-][A-Za-z0-9_./-]*\.[A-Za-z0-9_-]+(?:#[A-Za-z0-9_:/.-]+)?)/g;

export function analyzeSourceReferences(
  text: string,
  lookup: SourceTargetLookup,
): SourceReference[] {
  const references: SourceReference[] = [];

  for (const match of text.matchAll(CRIV_SOURCE_DIRECTIVE)) {
    const target = match[1];
    if (target) {
      references.push(
        referenceFor(lookup, "criv-source", target, match.index + match[0].indexOf(target), true),
      );
    }
  }

  for (const match of text.matchAll(SOURCE_WIKILINK)) {
    const typed = Boolean(match[1]);
    const target = match[2];
    if (!target || (!typed && !looksLikeSourceTarget(target))) {
      continue;
    }
    const start = match.index + match[0].indexOf(target);
    references.push(
      referenceFor(
        lookup,
        typed ? "typed-source-wikilink" : "legacy-wikilink",
        typed ? `source:${target}` : target,
        start,
        true,
      ),
    );
  }

  for (const match of text.matchAll(SELECTOR_CANDIDATE)) {
    const candidate = match[2];
    if (!candidate) {
      continue;
    }
    const start = match.index + match[0].length - candidate.length;
    if (
      resolveSourceTarget(lookup, candidate).kind === "unresolved" ||
      overlaps(references, start, start + candidate.length)
    ) {
      continue;
    }
    references.push(referenceFor(lookup, "selector", candidate, start, false));
  }

  return references.sort((left, right) => left.start - right.start);
}

export function referenceAtOffset(
  references: readonly SourceReference[],
  offset: number,
): SourceReference | undefined {
  return references.find((reference) => reference.start <= offset && offset <= reference.end);
}

/** Where to open a source target: the canonical node's file at the requested or node lines. */
export function planSourceTargetOpen(
  lookup: SourceTargetLookup,
  target: string,
): SourceTargetOpenPlan {
  const result = resolveSourceTarget(lookup, target);
  if (result.kind !== "resolved") {
    return result;
  }
  const path = safeVaultPath(sourceTargetParts(result.node.path ?? result.canonical_target).path);
  if (!path) {
    return { kind: "unresolved" };
  }
  return {
    kind: "resolved",
    path,
    lines: result.lines ?? parseLineFragment(result.node.line_range),
  };
}

export function sourceReferenceDiagnostic(reference: SourceReference): string | undefined {
  switch (reference.resolutionKind) {
    case "ambiguous":
      return ambiguousSourceTargetMessage(
        reference.target,
        reference.candidates ?? [],
        reference.totalCandidateCount ?? 0,
      );
    case "unresolved":
      return reference.shouldDiagnoseUnresolved
        ? `Unresolved criv source target: ${reference.target}.`
        : undefined;
    case "resolved":
      if (!reference.legacy) {
        return undefined;
      }
      return reference.canonicalTarget === sourceTargetParts(reference.target).body
        ? "Legacy source Wikilink; use an AST-aware source selector outside Wikilinks."
        : `Legacy source target; use AST-aware source selector ${reference.canonicalTarget}.`;
  }
}

export function sourceReferenceDiagnosticCode(reference: SourceReference): string {
  switch (reference.resolutionKind) {
    case "ambiguous":
      return "ambiguous-source-target";
    case "unresolved":
      return "unresolved-source-target";
    case "resolved":
      return "legacy-source-target";
  }
}

export function appendSourceHoverContents(
  contents: MarkdownSink,
  reference: SourceReference,
): void {
  if (reference.resolutionKind === "ambiguous") {
    contents.appendText(
      ambiguousSourceTargetMessage(
        reference.target,
        reference.candidates ?? [],
        reference.totalCandidateCount ?? 0,
      ),
    );
    return;
  }
  if (reference.resolutionKind !== "resolved" || !reference.canonicalTarget) {
    contents.appendText(`Unresolved criv source target: ${reference.target}.`);
    return;
  }

  const node = reference.node;
  contents.appendMarkdown("`");
  contents.appendText(reference.canonicalTarget);
  contents.appendMarkdown("`");
  if (node?.label) {
    contents.appendMarkdown("\n\n");
    contents.appendText(node.label);
  }
  if (node?.kind) {
    contents.appendMarkdown("\n\nKind: `");
    contents.appendText(node.kind);
    contents.appendMarkdown("`");
  }
  if (node?.line_range) {
    contents.appendMarkdown("\n\nRange: `");
    contents.appendText(node.line_range);
    contents.appendMarkdown("`");
  }
  if (reference.legacy) {
    contents.appendText("\n\nLegacy source link; prefer the AST-aware selector above.");
  }
}

export function completionToken(text: string, offset: number): { query: string; start: number } {
  const match = /[A-Za-z0-9_./:#-]*$/.exec(text.slice(0, offset));
  const query = match?.[0] ?? "";
  return { query, start: offset - query.length };
}

function referenceFor(
  lookup: SourceTargetLookup,
  kind: SourceReferenceKind,
  target: string,
  start: number,
  shouldDiagnoseUnresolved: boolean,
): SourceReference {
  const resolved = resolveSourceTarget(lookup, target);
  return {
    kind,
    target,
    canonicalTarget: resolved.kind === "resolved" ? resolved.canonical_target : undefined,
    node: resolved.kind === "resolved" ? resolved.node : undefined,
    resolutionKind: resolved.kind,
    candidates: resolved.kind === "ambiguous" ? resolved.candidates : undefined,
    totalCandidateCount: resolved.kind === "ambiguous" ? resolved.total_candidate_count : undefined,
    start,
    end: start + target.length,
    legacy: kind === "legacy-wikilink" || kind === "typed-source-wikilink",
    shouldDiagnoseUnresolved,
  };
}

function looksLikeSourceTarget(target: string): boolean {
  return target.includes("/") || target.includes(".");
}

function overlaps(references: readonly SourceReference[], start: number, end: number): boolean {
  return references.some((reference) => start < reference.end && end > reference.start);
}

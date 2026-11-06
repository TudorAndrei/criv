import {
  looksLikeSourceTarget,
  resolvedSourceLocation,
  resolveSourceTarget,
  sourceTargetParts,
  sourceTargetProblem,
  wikilinks,
  type CrivSourceTargetLookupResult,
  type SourceLocation,
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
  resolution: SourceTargetResolution;
  start: number;
  end: number;
  legacy: boolean;
  shouldDiagnoseUnresolved: boolean;
}

export type SourceTargetLookup = (target: string) => CrivSourceTargetLookupResult;

export type SourceTargetOpenPlan =
  | ({ kind: "resolved" } & SourceLocation)
  | Exclude<SourceTargetResolution, { kind: "resolved" }>;

export interface MarkdownSink {
  appendMarkdown(value: string): void;
  appendText(value: string): void;
}

const CRIV_SOURCE_DIRECTIVE = /\bcriv:source\s+([^\s"',)]+)/g;
const SELECTOR_CANDIDATE =
  /(^|[^\w./:#-])([A-Za-z0-9_.-][A-Za-z0-9_./-]*\.[A-Za-z0-9_-]+(?:#[A-Za-z0-9_:/.-]+)?)/g;
const SOURCE_PREFIX = "source:";

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

  for (const link of wikilinks(text)) {
    const typed = link.target.startsWith(SOURCE_PREFIX);
    if (!link.target || (!typed && !looksLikeSourceTarget(link.target))) {
      continue;
    }
    references.push(
      referenceFor(
        lookup,
        typed ? "typed-source-wikilink" : "legacy-wikilink",
        link.target,
        text.indexOf(link.target, link.start),
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
    if (overlaps(references, start, start + candidate.length)) {
      continue;
    }
    const reference = referenceFor(lookup, "selector", candidate, start, false);
    if (reference.resolution.kind !== "unresolved") {
      references.push(reference);
    }
  }

  return references.sort((left, right) => left.start - right.start);
}

export function referenceAtOffset(
  references: readonly SourceReference[],
  offset: number,
): SourceReference | undefined {
  return references.find((reference) => reference.start <= offset && offset <= reference.end);
}

export function planSourceTargetOpen(
  lookup: SourceTargetLookup,
  target: string,
): SourceTargetOpenPlan {
  const result = resolveSourceTarget(lookup, target);
  if (result.kind !== "resolved") {
    return result;
  }
  const location = resolvedSourceLocation(result);
  return location ? { kind: "resolved", ...location } : { kind: "unresolved" };
}

export function sourceReferenceDiagnostic(reference: SourceReference): string | undefined {
  const { resolution } = reference;
  if (resolution.kind === "unresolved" && !reference.shouldDiagnoseUnresolved) {
    return undefined;
  }
  if (resolution.kind !== "resolved") {
    return sourceTargetProblem(reference.target, resolution);
  }
  if (!reference.legacy) {
    return undefined;
  }
  return resolution.canonical_target === sourceTargetParts(reference.target).body
    ? "Legacy source Wikilink; use an AST-aware source selector outside Wikilinks."
    : `Legacy source target; use AST-aware source selector ${resolution.canonical_target}.`;
}

export function sourceReferenceDiagnosticCode(reference: SourceReference): string {
  switch (reference.resolution.kind) {
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
  const { resolution } = reference;
  if (resolution.kind !== "resolved") {
    contents.appendText(sourceTargetProblem(reference.target, resolution));
    return;
  }

  const { node } = resolution;
  contents.appendMarkdown("`");
  contents.appendText(resolution.canonical_target);
  contents.appendMarkdown("`");
  if (node.label) {
    contents.appendMarkdown("\n\n");
    contents.appendText(node.label);
  }
  contents.appendMarkdown("\n\nKind: `");
  contents.appendText(node.kind);
  contents.appendMarkdown("`");
  if (node.line_range) {
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
  return {
    kind,
    target,
    resolution: resolveSourceTarget(lookup, target),
    start,
    end: start + target.length,
    legacy: kind === "legacy-wikilink" || kind === "typed-source-wikilink",
    shouldDiagnoseUnresolved,
  };
}

function overlaps(references: readonly SourceReference[], start: number, end: number): boolean {
  return references.some((reference) => start < reference.end && end > reference.start);
}

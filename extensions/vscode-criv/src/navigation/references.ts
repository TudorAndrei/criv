import {
  looksLikeSourceTarget,
  resolvedSourceLocation,
  resolveSourceTarget,
  SOURCE_WIKILINK,
  sourceTargetProblem,
  sourceWikilinkWarning,
  wikilinks,
  type CrivSourceTargetLookupResult,
  type SourceLocation,
  type SourceTargetResolution,
} from "@criv/editor-state";

/**
 * A source target in Markdown: a bare AST-aware selector, which is linked only
 * when it resolves, or a Wikilink that names a file or a `source:` target.
 */
export interface SourceReference {
  kind: "selector" | "wikilink";
  target: string;
  resolution: SourceTargetResolution;
  start: number;
  end: number;
}

export type SourceTargetLookup = (target: string) => CrivSourceTargetLookupResult;

export type SourceTargetOpenPlan =
  | ({ kind: "resolved" } & SourceLocation)
  | Exclude<SourceTargetResolution, { kind: "resolved" }>;

export interface MarkdownSink {
  appendMarkdown(value: string): void;
  appendText(value: string): void;
}

const SELECTOR_CANDIDATE =
  /(^|[^\w./:#-])([A-Za-z0-9_.-][A-Za-z0-9_./-]*\.[A-Za-z0-9_-]+(?:#[A-Za-z0-9_:/.-]+)?)/g;
const SOURCE_PREFIX = "source:";

export function analyzeSourceReferences(
  text: string,
  lookup: SourceTargetLookup,
): SourceReference[] {
  const references: SourceReference[] = [];

  for (const link of wikilinks(text)) {
    if (
      !link.target ||
      (!link.target.startsWith(SOURCE_PREFIX) && !looksLikeSourceTarget(link.target))
    ) {
      continue;
    }
    references.push(
      referenceFor(lookup, "wikilink", link.target, text.indexOf(link.target, link.start)),
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
    const reference = referenceFor(lookup, "selector", candidate, start);
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
  if (resolution.kind === "resolved") {
    return reference.kind === "wikilink" ? sourceWikilinkWarning(resolution) : undefined;
  }
  return sourceTargetProblem(reference.target, resolution);
}

export function sourceReferenceDiagnosticCode(reference: SourceReference): string {
  switch (reference.resolution.kind) {
    case "ambiguous":
      return "ambiguous-source-target";
    case "unresolved":
      return "unresolved-source-target";
    case "resolved":
      return SOURCE_WIKILINK;
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
  if (reference.kind === "wikilink") {
    contents.appendMarkdown("\n\n");
    contents.appendText(sourceWikilinkWarning(resolution));
  }
}

export function completionToken(text: string, offset: number): { query: string; start: number } {
  const match = /[A-Za-z0-9_./:#-]*$/.exec(text.slice(0, offset));
  const query = match?.[0] ?? "";
  return { query, start: offset - query.length };
}

function referenceFor(
  lookup: SourceTargetLookup,
  kind: SourceReference["kind"],
  target: string,
  start: number,
): SourceReference {
  return {
    kind,
    target,
    resolution: resolveSourceTarget(lookup, target),
    start,
    end: start + target.length,
  };
}

function overlaps(references: readonly SourceReference[], start: number, end: number): boolean {
  return references.some((reference) => start < reference.end && end > reference.start);
}

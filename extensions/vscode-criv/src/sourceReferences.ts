import type { CrivGraphNode } from "./wasm";

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
  start: number;
  end: number;
  legacy: boolean;
  shouldDiagnoseUnresolved: boolean;
}

export type SourceTargetLookup = (target: string) => CrivGraphNode | undefined;

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
    if (!target) {
      continue;
    }
    references.push(
      referenceFor(lookup, "criv-source", target, match.index + match[0].indexOf(target), true),
    );
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
    const resolved = resolveSourceTarget(lookup, candidate);
    if (!resolved || overlaps(references, start, start + candidate.length)) {
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

export function resolveSourceTarget(
  lookup: SourceTargetLookup,
  target: string,
): { canonicalTarget: string; node?: CrivGraphNode } | undefined {
  const normalized = stripSourcePrefix(target);
  const node = lookup(normalized);
  const canonicalTarget = node?.source_target ?? node?.path;
  if (!canonicalTarget) {
    return undefined;
  }
  return { canonicalTarget, node };
}

export function sourceReferenceDiagnostic(reference: SourceReference): string | undefined {
  if (reference.legacy && reference.canonicalTarget) {
    return reference.canonicalTarget === stripSourcePrefix(reference.target)
      ? "Legacy source Wikilink; use an AST-aware source selector outside Wikilinks."
      : `Legacy source target; use AST-aware source selector ${reference.canonicalTarget}.`;
  }

  if (reference.shouldDiagnoseUnresolved && !reference.canonicalTarget) {
    return `Unresolved criv source target: ${reference.target}.`;
  }

  return undefined;
}

export function appendSourceHoverContents(
  contents: MarkdownSink,
  reference: SourceReference,
): void {
  if (!reference.canonicalTarget) {
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
  const prefix = text.slice(0, offset);
  const match = /[A-Za-z0-9_./:#-]*$/.exec(prefix);
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
  const legacy = kind === "legacy-wikilink" || kind === "typed-source-wikilink";
  return {
    kind,
    target,
    canonicalTarget: resolved?.canonicalTarget,
    node: resolved?.node,
    start,
    end: start + target.length,
    legacy,
    shouldDiagnoseUnresolved,
  };
}

function stripSourcePrefix(target: string): string {
  return target.startsWith("source:") ? target.slice("source:".length) : target;
}

function looksLikeSourceTarget(target: string): boolean {
  return (target.includes("/") || target.includes(".")) && !target.includes(" ");
}

function overlaps(references: readonly SourceReference[], start: number, end: number): boolean {
  return references.some((reference) => start < reference.end && end > reference.start);
}

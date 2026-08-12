import { parseLineFragment, parseSourceTarget, type ParsedSourceTarget } from "./sourceTarget";
import type {
  CrivGraphNode,
  CrivSourceTargetCandidate,
  CrivSourceTargetLookupResult,
} from "./wasm";

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
  resolutionKind: "resolved" | "unresolved" | "ambiguous" | "malformed";
  candidates?: CrivSourceTargetCandidate[];
  totalCandidateCount?: number;
  start: number;
  end: number;
  legacy: boolean;
  shouldDiagnoseUnresolved: boolean;
}

export type SourceTargetLookup = (target: string) => CrivSourceTargetLookupResult;

export type SourceTargetResolution = CrivSourceTargetLookupResult | { kind: "malformed" };

export type SourceTargetOpenPlan =
  | { kind: "resolved"; target: ParsedSourceTarget }
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
    if (
      resolved.kind === "unresolved" ||
      resolved.kind === "malformed" ||
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

export function resolveSourceTarget(
  lookup: SourceTargetLookup,
  target: string,
): SourceTargetResolution {
  const normalized = stripSourcePrefix(target);
  const fragmentIndex = normalized.indexOf("#");
  let lookupTarget = normalized;
  if (fragmentIndex >= 0) {
    const path = normalized.slice(0, fragmentIndex);
    const fragment = normalized.slice(fragmentIndex + 1);
    if (/^l/i.test(fragment)) {
      if (!parseLineFragment(fragment)) {
        return { kind: "malformed" };
      }
      lookupTarget = path;
    }
  }
  const result = lookup(lookupTarget);
  if (result.kind === "resolved") {
    return result;
  }
  if (result.kind === "ambiguous") {
    return result;
  }
  if (result.kind === "unresolved") {
    return result;
  }
  return { kind: "unresolved" };
}

export function planSourceTargetOpen(
  lookup: SourceTargetLookup,
  target: string,
): SourceTargetOpenPlan {
  const result = resolveSourceTarget(lookup, target);
  if (result.kind === "unresolved") {
    return result;
  }
  if (result.kind === "ambiguous") {
    return result;
  }
  if (result.kind === "malformed") {
    return result;
  }
  if (result.kind !== "resolved") {
    return { kind: "unresolved" };
  }

  const canonical = parseSourceTarget(result.node.path ?? result.canonical_target);
  if (!canonical) {
    return { kind: "malformed" };
  }
  const requested = parseSourceTarget(target);
  if (requested?.line !== undefined) {
    canonical.fragment = requested.fragment;
    canonical.line = requested.line;
    canonical.endLine = requested.endLine;
  }
  return { kind: "resolved", target: canonical };
}

export function sourceReferenceDiagnostic(reference: SourceReference): string | undefined {
  if (reference.resolutionKind === "ambiguous") {
    return ambiguousSourceTargetMessage(
      reference.target,
      reference.candidates ?? [],
      reference.totalCandidateCount ?? 0,
    );
  }
  if (reference.resolutionKind === "malformed") {
    return `Malformed criv source target: ${reference.target}.`;
  }
  if (reference.legacy && reference.resolutionKind === "resolved") {
    return reference.canonicalTarget === stripSourcePrefix(reference.target)
      ? "Legacy source Wikilink; use an AST-aware source selector outside Wikilinks."
      : `Legacy source target; use AST-aware source selector ${reference.canonicalTarget}.`;
  }

  if (reference.shouldDiagnoseUnresolved && reference.resolutionKind === "unresolved") {
    return `Unresolved criv source target: ${reference.target}.`;
  }

  return undefined;
}

export function sourceReferenceDiagnosticCode(reference: SourceReference): string {
  if (reference.resolutionKind === "ambiguous") {
    return "ambiguous-source-target";
  }
  if (reference.resolutionKind === "malformed") {
    return "malformed-source-target";
  }
  if (reference.resolutionKind === "unresolved") {
    return "unresolved-source-target";
  }
  return "legacy-source-target";
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
  if (reference.resolutionKind === "malformed") {
    contents.appendText(`Malformed criv source target: ${reference.target}.`);
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
    canonicalTarget: resolved.kind === "resolved" ? resolved.canonical_target : undefined,
    node: resolved.kind === "resolved" ? resolved.node : undefined,
    resolutionKind: resolved.kind,
    candidates: resolved.kind === "ambiguous" ? resolved.candidates : undefined,
    totalCandidateCount: resolved.kind === "ambiguous" ? resolved.total_candidate_count : undefined,
    start,
    end: start + target.length,
    legacy,
    shouldDiagnoseUnresolved,
  };
}

export function ambiguousSourceTargetMessage(
  target: string,
  candidates: readonly CrivSourceTargetCandidate[],
  totalCandidateCount: number,
): string {
  const details = candidates.map((candidate) => {
    const base = `${candidate.canonical_target} (${candidate.kind}: ${candidate.label})`;
    const duplicateTarget =
      candidates.filter((other) => other.canonical_target === candidate.canonical_target).length >
      1;
    return duplicateTarget ? `${base} [${candidate.node_id}]` : base;
  });
  const remaining = totalCandidateCount - candidates.length;
  const suffix = remaining > 0 ? `; ${remaining} more` : "";
  return `Ambiguous criv source target ${target}: ${details.join("; ")}${suffix}.`;
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

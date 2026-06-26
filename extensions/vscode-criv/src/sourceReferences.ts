import type { CrivStateSnapshot } from "./stateModel";
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

export interface SourceTargetIndex {
  readonly targets: ReadonlySet<string>;
  readonly nodesByTarget: ReadonlyMap<string, CrivGraphNode>;
  readonly canonicalByLegacy: ReadonlyMap<string, string>;
}

export interface MarkdownSink {
  appendMarkdown(value: string): void;
  appendText(value: string): void;
}

interface MutableSourceTargetIndex {
  targets: Set<string>;
  nodesByTarget: Map<string, CrivGraphNode>;
  canonicalByLegacy: Map<string, string>;
}

const CRIV_SOURCE_DIRECTIVE = /\bcriv:source\s+([^\s"',)]+)/g;
const SOURCE_WIKILINK = /\[\[(source:)?([^\]|\s]+)(?:\|[^\]]*)?\]\]/g;
const SELECTOR_CANDIDATE =
  /(^|[^\w./:#-])([A-Za-z0-9_.-][A-Za-z0-9_./-]*\.[A-Za-z0-9_-]+(?:#[A-Za-z0-9_:/.-]+)?)/g;

export function buildSourceTargetIndex(snapshot: CrivStateSnapshot): SourceTargetIndex {
  const index: MutableSourceTargetIndex = {
    targets: new Set<string>(),
    nodesByTarget: new Map<string, CrivGraphNode>(),
    canonicalByLegacy: new Map<string, string>(),
  };

  for (const source of snapshot.sources) {
    addTarget(index, source.path);
  }

  for (const node of snapshot.graphNodes) {
    const target = node.source_target ?? node.path;
    if (!target) {
      continue;
    }
    addTarget(index, target, node);
    addTarget(index, node.id, node);
    addLegacySymbolAliases(index, target, node);
  }

  return index;
}

export function analyzeSourceReferences(text: string, index: SourceTargetIndex): SourceReference[] {
  const references: SourceReference[] = [];

  for (const match of text.matchAll(CRIV_SOURCE_DIRECTIVE)) {
    const target = match[1];
    if (!target) {
      continue;
    }
    references.push(
      referenceFor(index, "criv-source", target, match.index + match[0].indexOf(target), true),
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
        index,
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
    const resolved = resolveSourceTarget(index, candidate);
    if (!resolved || overlaps(references, start, start + candidate.length)) {
      continue;
    }
    references.push(referenceFor(index, "selector", candidate, start, false));
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
  index: SourceTargetIndex,
  target: string,
): { canonicalTarget: string; node?: CrivGraphNode } | undefined {
  const normalized = stripSourcePrefix(target);
  const canonicalTarget = index.targets.has(normalized)
    ? normalized
    : index.canonicalByLegacy.get(normalized);
  if (!canonicalTarget) {
    return undefined;
  }
  return { canonicalTarget, node: index.nodesByTarget.get(canonicalTarget) };
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
  index: SourceTargetIndex,
  kind: SourceReferenceKind,
  target: string,
  start: number,
  shouldDiagnoseUnresolved: boolean,
): SourceReference {
  const resolved = resolveSourceTarget(index, target);
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

function addTarget(index: MutableSourceTargetIndex, target: string, node?: CrivGraphNode): void {
  index.targets.add(target);
  if (node) {
    index.nodesByTarget.set(target, node);
  }
}

function addLegacySymbolAliases(
  index: MutableSourceTargetIndex,
  target: string,
  node: CrivGraphNode,
): void {
  const [path, fragment] = splitFragment(target);
  if (!fragment) {
    return;
  }

  const shortName = fragment.split(":").at(-1);
  if (shortName) {
    addLegacyAlias(index, `${path}#${shortName}`, target);
  }
  if (node.label) {
    addLegacyAlias(index, `${path}#${node.label}`, target);
  }
}

function addLegacyAlias(
  index: MutableSourceTargetIndex,
  legacyTarget: string,
  canonicalTarget: string,
): void {
  const existing = index.canonicalByLegacy.get(legacyTarget);
  if (existing && existing !== canonicalTarget) {
    index.canonicalByLegacy.delete(legacyTarget);
    return;
  }
  index.canonicalByLegacy.set(legacyTarget, canonicalTarget);
}

function splitFragment(target: string): [string, string | undefined] {
  const index = target.indexOf("#");
  return index === -1 ? [target, undefined] : [target.slice(0, index), target.slice(index + 1)];
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

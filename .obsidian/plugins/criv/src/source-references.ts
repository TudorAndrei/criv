import {
  EditorSuggest,
  Notice,
  type Editor,
  type EditorPosition,
  type EditorSuggestContext,
  type EditorSuggestTriggerInfo,
  type MarkdownPostProcessorContext,
} from "obsidian";
import type { App } from "obsidian";
import { RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  type EditorView,
  type PluginValue,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import {
  addTarget,
  addTextTargets,
  crivLinkRanges,
  decodeSourceLinkTarget,
  looksLikeSourceOrPattern,
  patternTooltip,
  resolvePattern,
  resolveSourceResult,
  safeVaultPath,
  sourceTooltip,
  type CrivState,
  type LinkedSource,
  type SourceResolver,
} from "./core";
import type { StatePort } from "./ports";
import {
  positionHoverPreview,
  readSourcePreview,
  renderPreview,
  renderPreviewError,
} from "./source-preview";
import type { CrivSelectorSuggestion } from "./wasm";

const LINK_TARGET_SELECTOR = [
  "[data-href]",
  "a.internal-link",
  "a[href]",
  ".internal-link",
  ".cm-hmd-internal-link",
  ".cm-link",
  ".cm-url",
  ".cm-underline",
].join(",");

interface SourceSuggestionItem {
  insertText: string;
  label: string;
  path: string;
  detail?: string;
}

export class ObsidianSourceReferencesOwner {
  private hoverEl: HTMLElement | null = null;
  private hoverSourceKey: string | null = null;
  private hoverRequest = 0;

  constructor(
    private readonly app: App,
    private readonly state: StatePort,
    private readonly openExternal: (path: string) => void,
  ) {}

  createSuggest(): EditorSuggest<SourceSuggestionItem> {
    return new CrivSourceSuggest(this.app, this.state);
  }

  editorExtension() {
    return ViewPlugin.fromClass<CrivEditorDriftPlugin, StatePort>(CrivEditorDriftPlugin, {
      decorations: (value) => value.decorations,
    }).of(this.state);
  }

  async decorateLinks(el: HTMLElement, _ctx: MarkdownPostProcessorContext): Promise<void> {
    const state = await this.state.getState();
    if (!state) {
      return;
    }
    const candidates = Array.from(
      el.querySelectorAll("[data-href], a.internal-link, a[href]"),
    ) as HTMLElement[];
    for (const anchor of candidates) {
      const parsedTargets = linkTargets(anchor);
      if (parsedTargets.malformed) {
        anchor.addClass("criv-warning");
        anchor.setAttribute("title", "Malformed criv source target");
        continue;
      }
      const sourceResult = resolveSourceResultFromTargets(
        this.state.cachedSourceResolver(),
        parsedTargets.targets,
      );
      const pattern = resolvePatternFromElement(state, anchor);
      if (sourceResult.kind === "resolved") {
        anchor.addClass("criv-source-ref");
        anchor.setAttribute("title", sourceTooltip(sourceResult.source));
        continue;
      }
      if (sourceResult.kind === "ambiguous") {
        anchor.addClass("criv-warning");
        anchor.setAttribute(
          "title",
          ambiguousSourceMessage(
            parsedTargets.targets[0] ?? "",
            sourceResult.candidates,
            sourceResult.totalCandidateCount,
          ),
        );
        continue;
      }
      if (pattern) {
        anchor.addClass("criv-pattern-ref");
        anchor.setAttribute("title", patternTooltip(state, pattern));
        continue;
      }
      const target = parsedTargets.targets[0] ?? "";
      if (looksLikeSourceOrPattern(target)) {
        anchor.addClass("criv-warning");
        anchor.setAttribute("title", "Unresolved criv reference");
      }
    }
  }

  async handleDocumentMouseOver(event: MouseEvent): Promise<void> {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const link = target?.closest(LINK_TARGET_SELECTOR) as HTMLElement | null;
    if (!link || link.closest(".criv-hover-preview")) {
      return;
    }
    if (!(await this.state.getState())) {
      return;
    }
    const source = resolveSourceFromElement(this.state.cachedSourceResolver(), link);
    if (!source) {
      return;
    }
    link.addClass("criv-source-ref");
    link.setAttribute("title", sourceTooltip(source));
    await this.showHoverPreview(event, source);
  }

  handleDocumentMouseOut(event: MouseEvent): void {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const link = target?.closest(LINK_TARGET_SELECTOR) as HTMLElement | null;
    if (!link) {
      return;
    }
    const related = event.relatedTarget instanceof Node ? event.relatedTarget : null;
    if (related && link.contains(related)) {
      return;
    }
    this.hideHoverPreview();
  }

  openValidatedSource(target: string): void {
    const result = resolveSourceResult(this.state.cachedSourceResolver(), target);
    if (result.kind === "resolved") {
      this.openExternal(result.source.entry.path);
      return;
    }
    new Notice(`Could not resolve criv source target ${target}.`);
  }

  dispose(): void {
    this.hideHoverPreview();
  }

  private async showHoverPreview(event: MouseEvent, source: LinkedSource): Promise<void> {
    const sourceKey = `${source.entry.path}#${source.fragment ?? ""}`;
    if (this.hoverEl && this.hoverSourceKey === sourceKey) {
      positionHoverPreview(this.hoverEl, event);
      return;
    }
    this.hideHoverPreview();
    const request = ++this.hoverRequest;
    const preview = createDiv({ cls: "criv-hover-preview" });
    preview.createDiv({ cls: "criv-preview-path", text: source.entry.path });
    preview.createDiv({ cls: "criv-preview-loading", text: "Loading preview..." });
    document.body.appendChild(preview);
    positionHoverPreview(preview, event);
    this.hoverEl = preview;
    this.hoverSourceKey = sourceKey;

    try {
      const data = await readSourcePreview(this.app, source);
      if (request !== this.hoverRequest || this.hoverEl !== preview) {
        return;
      }
      renderPreview(preview, data, false);
    } catch {
      if (request !== this.hoverRequest || this.hoverEl !== preview) {
        return;
      }
      renderPreviewError(preview, source.entry.path);
    }
  }

  private hideHoverPreview(): void {
    this.hoverRequest += 1;
    this.hoverEl?.remove();
    this.hoverEl = null;
    this.hoverSourceKey = null;
  }
}

class CrivSourceSuggest extends EditorSuggest<SourceSuggestionItem> {
  constructor(
    app: App,
    private readonly state: StatePort,
  ) {
    super(app);
  }

  onTrigger(cursor: EditorPosition, editor: Editor): EditorSuggestTriggerInfo | null {
    const line = editor.getLine(cursor.line).slice(0, cursor.ch);
    const open = line.lastIndexOf("[[");
    if (open === -1 || line.slice(open).includes("]]")) {
      return null;
    }
    const query = line.slice(open + 2);
    if (query.includes(" ") || query.startsWith("match:")) {
      return null;
    }
    return {
      start: { line: cursor.line, ch: open + 2 },
      end: cursor,
      query,
    };
  }

  async getSuggestions(context: EditorSuggestContext): Promise<SourceSuggestionItem[]> {
    if (!(await this.state.getState())) {
      return [];
    }
    try {
      return sourceSuggestionItemsFromWasm(this.state.suggestSourceSelectors(context.query, 20));
    } catch (error) {
      this.state.recordWasmFailure(error);
      return [];
    }
  }

  renderSuggestion(value: SourceSuggestionItem, el: HTMLElement): void {
    el.createDiv({ text: value.label });
    if (value.detail) {
      el.createDiv({ cls: "criv-source-suggestion-detail", text: value.detail });
    }
  }

  selectSuggestion(value: SourceSuggestionItem): void {
    if (this.context) {
      this.context.editor.replaceRange(value.insertText, this.context.start, this.context.end);
    }
  }
}

class CrivEditorDriftPlugin implements PluginValue {
  decorations: DecorationSet;

  constructor(
    view: EditorView,
    private readonly state: StatePort,
  ) {
    this.decorations = this.buildDecorations(view);
  }

  update(update: ViewUpdate): void {
    if (update.docChanged || update.viewportChanged) {
      this.decorations = this.buildDecorations(update.view);
    }
  }

  private buildDecorations(view: EditorView): DecorationSet {
    const state = this.state.cachedState();
    if (!state) {
      return Decoration.none;
    }
    const builder = new RangeSetBuilder<Decoration>();
    for (const { from, to } of view.visibleRanges) {
      const text = view.state.sliceDoc(from, to);
      for (const range of crivLinkRanges(text, state, this.state.cachedSourceResolver())) {
        if (range.status === "resolved") {
          continue;
        }
        const title =
          range.status === "ambiguous"
            ? ambiguousSourceMessage(
                range.target,
                range.candidates ?? [],
                range.totalCandidateCount ?? 0,
              )
            : range.status === "malformed"
              ? "Malformed criv source target"
              : "Unresolved criv reference";
        builder.add(
          from + range.from,
          from + range.to,
          Decoration.mark({
            class: "criv-editor-warning",
            attributes: { "data-criv-target": range.target, title },
          }),
        );
      }
    }
    return builder.finish();
  }
}

function sourceSuggestionItemsFromWasm(items: CrivSelectorSuggestion[]): SourceSuggestionItem[] {
  const suggestions: SourceSuggestionItem[] = [];
  for (const item of items) {
    const path = safeVaultPath(item.path);
    if (!path) {
      continue;
    }
    suggestions.push({
      insertText: item.target,
      label: item.label || item.target,
      path,
      detail: item.detail || item.kind,
    });
  }
  return suggestions;
}

function resolveSourceFromElement(
  resolver: SourceResolver,
  element: HTMLElement,
): LinkedSource | null {
  const parsed = linkTargets(element);
  if (parsed.malformed) {
    return null;
  }
  const result = resolveSourceResultFromTargets(resolver, parsed.targets);
  return result.kind === "resolved" ? result.source : null;
}

function resolveSourceResultFromTargets(
  resolver: SourceResolver,
  targets: string[],
): ReturnType<typeof resolveSourceResult> {
  for (const target of targets) {
    const result = resolveSourceResult(resolver, target);
    if (result.kind !== "unresolved") {
      return result;
    }
  }
  return { kind: "unresolved" };
}

function resolvePatternFromElement(state: CrivState, element: HTMLElement): string | null {
  for (const target of linkTargets(element).targets) {
    const pattern = resolvePattern(state, target);
    if (pattern) {
      return pattern;
    }
  }
  return null;
}

function linkTargets(element: HTMLElement): { targets: string[]; malformed: boolean } {
  const targets: string[] = [];
  const dataHref = element.getAttribute("data-href");
  if (dataHref) {
    addTarget(targets, dataHref);
  }
  const ariaLabel = element.getAttribute("aria-label");
  if (ariaLabel) {
    const match = ariaLabel.match(/(?:open|link|to)\s+(.+)$/i);
    if (match?.[1]) {
      addTarget(targets, match[1]);
    }
  }
  const href = element.getAttribute("href");
  if (href && !href.includes("://")) {
    const decoded = decodeSourceLinkTarget(href.replace(/^#/, ""));
    if (decoded === null) {
      return { targets: [], malformed: true };
    }
    addTarget(targets, decoded);
  }
  addTextTargets(targets, element.textContent);
  addTextTargets(targets, (element.closest(".cm-line") as HTMLElement | null)?.textContent);
  return { targets: Array.from(new Set(targets)), malformed: false };
}

function ambiguousSourceMessage(
  target: string,
  candidates: { canonical_target: string; kind: string; label: string; node_id: string }[],
  totalCandidateCount: number,
): string {
  const details = candidates.map((candidate) => {
    const base = `${candidate.canonical_target} (${candidate.kind}: ${candidate.label})`;
    const sameTargetCount = candidates.filter(
      (other) => other.canonical_target === candidate.canonical_target,
    ).length;
    return sameTargetCount > 1 ? `${base} [${candidate.node_id}]` : base;
  });
  const remaining = totalCandidateCount - candidates.length;
  const suffix = remaining > 0 ? `; ${remaining} more` : "";
  return `Ambiguous criv source target ${target}: ${details.join("; ")}${suffix}`;
}

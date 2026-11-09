import {
  EditorSuggest,
  type Editor,
  type EditorPosition,
  type EditorSuggestContext,
  type EditorSuggestTriggerInfo,
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
  patternTargetId,
  wikilinks,
  type CrivInitialProjections,
  type CrivSelectorSuggestion,
} from "@criv/editor-state";
import {
  crivLinkRanges,
  crivLinkStatus,
  patternTooltip,
  sourceTooltip,
  type LinkedSource,
} from "./model";
import type { StatePort } from "../ports";
import {
  positionHoverPreview,
  readSourcePreview,
  renderPreview,
  renderPreviewError,
} from "./preview";

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

export class ObsidianSourceReferencesOwner {
  private hoverEl: HTMLElement | null = null;
  private hoverSourceKey: string | null = null;
  private hoverRequest = 0;

  constructor(
    private readonly app: App,
    private readonly state: StatePort,
  ) {}

  createSuggest(): EditorSuggest<CrivSelectorSuggestion> {
    return new CrivSourceSuggest(this.app, this.state);
  }

  editorExtension() {
    return ViewPlugin.fromClass<CrivEditorDriftPlugin, StatePort>(CrivEditorDriftPlugin, {
      decorations: (value) => value.decorations,
    }).of(this.state);
  }

  async decorateLinks(el: HTMLElement): Promise<void> {
    const state = await this.state.load();
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
      const status = crivLinkStatus(state, this.state.sourceResolver(), parsedTargets.targets);
      switch (status?.kind) {
        case "source":
          anchor.addClass("criv-source-ref");
          anchor.setAttribute("title", sourceTooltip(status.source));
          break;
        case "pattern":
          anchor.addClass("criv-pattern-ref");
          anchor.setAttribute("title", patternTooltip(state, status.id));
          break;
        case "problem":
          anchor.addClass("criv-warning");
          anchor.setAttribute("title", status.message);
          break;
        case undefined:
          break;
      }
    }
  }

  async handleDocumentMouseOver(event: MouseEvent): Promise<void> {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const link = target?.closest(LINK_TARGET_SELECTOR) as HTMLElement | null;
    if (!link || link.closest(".criv-hover-preview")) {
      return;
    }
    const state = await this.state.load();
    const parsed = linkTargets(link);
    if (!state || parsed.malformed) {
      return;
    }
    const status = crivLinkStatus(state, this.state.sourceResolver(), parsed.targets);
    if (status?.kind !== "source") {
      return;
    }
    link.addClass("criv-source-ref");
    link.setAttribute("title", sourceTooltip(status.source));
    await this.showHoverPreview(event, status.source);
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

  dispose(): void {
    this.hideHoverPreview();
  }

  private async showHoverPreview(event: MouseEvent, source: LinkedSource): Promise<void> {
    const sourceKey = `${source.entry.path}#${source.lines?.start}-${source.lines?.end}`;
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

/**
 * Completes a source selector typed after `[[`. Selecting one replaces the
 * whole Wikilink with the bare selector, because `criv check` reserves
 * Wikilinks for document references (`sourceWikilinkWarning`).
 */
class CrivSourceSuggest extends EditorSuggest<CrivSelectorSuggestion> {
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
    if (query.includes(" ") || patternTargetId(query) !== undefined) {
      return null;
    }
    return {
      start: { line: cursor.line, ch: open },
      end: cursor,
      query,
    };
  }

  async getSuggestions(context: EditorSuggestContext): Promise<CrivSelectorSuggestion[]> {
    if (!(await this.state.load())) {
      return [];
    }
    return this.state.suggestSelectors(context.query, 20);
  }

  renderSuggestion(value: CrivSelectorSuggestion, el: HTMLElement): void {
    el.createDiv({ text: value.label || value.target });
    el.createDiv({ cls: "criv-source-suggestion-detail", text: value.detail || value.kind });
  }

  selectSuggestion(value: CrivSelectorSuggestion): void {
    if (!this.context) {
      return;
    }
    const { editor, start, end } = this.context;
    const closing = { line: end.line, ch: end.ch + 2 };
    editor.replaceRange(
      value.target,
      start,
      editor.getRange(end, closing) === "]]" ? closing : end,
    );
  }
}

/**
 * Marks unresolved and ambiguous links. A State reload reaches every editor
 * through `workspace.updateOptions()`, whose reconfiguration updates this
 * plugin without a document or viewport change, so it also rebuilds when the
 * loaded projections change.
 */
class CrivEditorDriftPlugin implements PluginValue {
  decorations: DecorationSet;
  private projections: CrivInitialProjections | null;

  constructor(
    view: EditorView,
    private readonly state: StatePort,
  ) {
    this.projections = state.current();
    this.decorations = this.buildDecorations(view);
  }

  update(update: ViewUpdate): void {
    const projections = this.state.current();
    if (update.docChanged || update.viewportChanged || projections !== this.projections) {
      this.projections = projections;
      this.decorations = this.buildDecorations(update.view);
    }
  }

  private buildDecorations(view: EditorView): DecorationSet {
    const state = this.projections;
    if (!state) {
      return Decoration.none;
    }
    const builder = new RangeSetBuilder<Decoration>();
    for (const { from, to } of view.visibleRanges) {
      const text = view.state.sliceDoc(from, to);
      for (const range of crivLinkRanges(text, state, this.state.sourceResolver())) {
        if (range.status.kind !== "problem") {
          continue;
        }
        builder.add(
          from + range.from,
          from + range.to,
          Decoration.mark({
            class: "criv-editor-warning",
            attributes: { "data-criv-target": range.target, title: range.status.message },
          }),
        );
      }
    }
    return builder.finish();
  }
}

/** Candidate link targets for a rendered link, from its attributes and its own text. */
function linkTargets(element: HTMLElement): { targets: string[]; malformed: boolean } {
  const targets: string[] = [];
  addTarget(targets, element.getAttribute("data-href"));
  addTarget(targets, element.getAttribute("aria-label")?.match(/(?:open|link|to)\s+(.+)$/i)?.[1]);
  const href = element.getAttribute("href");
  if (href && !href.includes("://")) {
    const decoded = decodeLinkTarget(href.replace(/^#/, ""));
    if (decoded === null) {
      return { targets: [], malformed: true };
    }
    addTarget(targets, decoded);
  }
  const text = element.textContent ?? "";
  addTarget(targets, text);
  for (const link of wikilinks(text)) {
    addTarget(targets, link.target);
  }
  return { targets: Array.from(new Set(targets)), malformed: false };
}

function addTarget(targets: string[], value: string | null | undefined): void {
  const target = value?.trim();
  if (target) {
    targets.push(target);
  }
}

function decodeLinkTarget(target: string): string | null {
  try {
    return decodeURIComponent(target);
  } catch {
    return null;
  }
}

import {
  Command,
  Editor,
  EditorPosition,
  EditorSuggest,
  EditorSuggestContext,
  EditorSuggestTriggerInfo,
  FileView,
  ItemView,
  MarkdownPostProcessorContext,
  Notice,
  Plugin,
  PluginSettingTab,
  Setting,
  TFile,
  WorkspaceLeaf,
} from "obsidian";
import mermaid from "mermaid";
import { instance as vizInstance, type Viz } from "@viz-js/viz";
import { RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  DecorationSet,
  EditorView,
  PluginValue,
  ViewPlugin,
  ViewUpdate,
} from "@codemirror/view";
import {
  CrivState,
  C4ArtifactSummary,
  FrontmatterPatternTarget,
  LinkedSource,
  SourceIndexEntry,
  addTarget,
  addTextTargets,
  crivLinkRanges,
  frontmatterPatternTargets,
  interpretState,
  linkedSourcesFromMarkdown,
  looksLikeSourceOrPattern,
  parseLineRange,
  parseC4Artifact,
  patternTooltip,
  renderErrorsMessage,
  resolvePattern,
  resolveSource,
  sanitizeDotSvg,
  safeVaultPath,
  sourceEntries,
  sourceSuggestions,
  sourceTooltip,
} from "./core";
import { summarizeState, suggestSourceSelectors, type CrivSelectorSuggestion } from "./wasm";

interface CrivSettings {
  statePath: string;
  externalEditorUrl: string;
}

const DEFAULT_SETTINGS: CrivSettings = {
  statePath: ".criv/state.json",
  externalEditorUrl: "vscode://file/{path}",
};
const EXPECTED_SCHEMA = "criv.state.v0";
const VIEW_TYPE = "criv-source-panel";
const C4_VIEW_TYPE = "criv-c4-view";
const PREVIEW_LINE_LIMIT = 80;
let c4RenderSequence = 0;
let c4DotRenderer: Promise<Viz> | null = null;
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

interface SourcePreview {
  path: string;
  language: string;
  text: string;
  startLine: number;
  truncated: boolean;
}

interface SourceSuggestionItem {
  insertText: string;
  label: string;
  path: string;
  detail?: string;
}

interface ObsidianCommandRegistry {
  commands?: Record<string, Command>;
}

export default class CrivPlugin extends Plugin {
  settings!: CrivSettings;
  private state: CrivState | null = null;
  private stateError: string | null = null;
  private hoverEl: HTMLElement | null = null;
  private hoverSourceKey: string | null = null;
  private hoverRequest = 0;

  async onload() {
    this.settings = Object.assign({}, DEFAULT_SETTINGS, await this.loadData());
    this.addRibbonIcon("network", "criv status", async () => this.showStatus());
    this.addCommand({
      id: "show-criv-status",
      name: "Show criv status",
      callback: async () => this.showStatus(),
    });
    this.addCommand({
      id: "open-criv-source-panel",
      name: "Open criv source panel",
      callback: async () => this.openSourcePanel(),
    });
    this.addCommand({
      id: "reload-criv-state",
      name: "Reload criv state",
      callback: async () => {
        await this.reloadState();
        await this.refreshSourcePanel();
      },
    });
    this.patchNativeSaveCommand();
    this.registerEditorExtension(crivDriftExtension(this));
    this.registerView(VIEW_TYPE, (leaf) => new CrivSourceView(leaf, this));
    this.registerView(C4_VIEW_TYPE, (leaf) => new CrivC4View(leaf));
    this.registerExtensions(["c4"], C4_VIEW_TYPE);
    this.registerMarkdownPostProcessor((el, ctx) => this.decorateLinks(el, ctx));
    this.registerDomEvent(document, "mouseover", (event) => this.handleDocumentMouseOver(event));
    this.registerDomEvent(document, "mouseout", (event) => this.handleDocumentMouseOut(event));
    this.registerEditorSuggest(new CrivSourceSuggest(this));
    this.registerEvent(
      this.app.workspace.on("active-leaf-change", () => this.refreshSourcePanel()),
    );
    this.registerEvent(this.app.metadataCache.on("changed", () => this.refreshSourcePanel()));
    this.addSettingTab(new CrivSettingTab(this.app, this));
    this.app.workspace.onLayoutReady(() => {
      void this.loadState().then(() => this.app.workspace.updateOptions());
      void this.ensureSourcePanel(false);
    });
  }

  onunload() {
    this.hideHoverPreview();
  }

  async showStatus() {
    const state = await this.readState();
    if (!state) {
      new Notice(`criv state is missing at ${this.settings.statePath}`);
      return;
    }
    if (state.schema !== EXPECTED_SCHEMA) {
      new Notice(`criv state schema ${state.schema ?? "unknown"} is not supported`);
      return;
    }

    new Notice(
      `criv ${state.schema}: ${state.node_count} nodes, ${state.edge_count} edges, ${state.source_count} source files`,
    );
  }

  async openSourcePanel() {
    await this.ensureSourcePanel(true);
  }

  async ensureSourcePanel(reveal: boolean): Promise<void> {
    const existing = this.sourcePanelLeaf();
    if (existing) {
      if (reveal) {
        this.app.workspace.revealLeaf(existing);
      }
      if (existing.view instanceof CrivSourceView) {
        await existing.view.render();
      }
      return;
    }

    const leaf = this.app.workspace.getRightLeaf(false);
    if (!leaf) {
      return;
    }
    await leaf.setViewState({ type: VIEW_TYPE, active: reveal });
    if (reveal) {
      this.app.workspace.revealLeaf(leaf);
    }
  }

  async readState() {
    const statePath = this.safeStatePath();
    if (!statePath) {
      return null;
    }
    try {
      const raw = await this.app.vault.adapter.read(statePath);
      return await summarizeState(raw);
    } catch {
      return null;
    }
  }

  async loadState(): Promise<CrivState | null> {
    const statePath = this.safeStatePath();
    if (!statePath) {
      this.state = null;
      this.stateError = `Invalid criv state path ${this.settings.statePath}.`;
      return null;
    }
    try {
      const raw = await this.app.vault.adapter.read(statePath);
      const interpreted = interpretState(raw, EXPECTED_SCHEMA);
      if ("error" in interpreted) {
        this.state = null;
        this.stateError =
          interpreted.kind === "parse"
            ? `Could not read ${statePath}: ${interpreted.error}`
            : interpreted.error;
        return null;
      }
      this.state = interpreted.state;
      this.stateError = null;
      return interpreted.state;
    } catch (error) {
      this.state = null;
      this.stateError = `Could not read ${statePath}: ${errorMessage(error)}`;
      return null;
    }
  }

  async getState(): Promise<CrivState | null> {
    return this.state ?? (await this.loadState());
  }

  stateStatus(): string {
    return this.stateError ?? `criv state is unavailable at ${this.settings.statePath}.`;
  }

  cachedState(): CrivState | null {
    return this.state;
  }

  async reloadState(): Promise<CrivState | null> {
    const state = await this.loadState();
    this.app.workspace.updateOptions();
    return state;
  }

  async refreshSourcePanel(): Promise<void> {
    await this.reloadState();
    const leaf = this.sourcePanelLeaf();
    if (leaf?.view instanceof CrivSourceView) {
      await leaf.view.render();
    }
  }

  private sourcePanelLeaf(): WorkspaceLeaf | null {
    const [first, ...duplicates] = this.app.workspace.getLeavesOfType(VIEW_TYPE);
    for (const duplicate of duplicates) {
      duplicate.detach();
    }
    return first ?? null;
  }

  frontmatterPatternTargets(state: CrivState): FrontmatterPatternTarget[] {
    const file = this.app.workspace.getActiveFile();
    if (!file) {
      return [];
    }
    const frontmatter = this.app.metadataCache.getFileCache(file)?.frontmatter;
    return frontmatterPatternTargets(frontmatter, state);
  }

  async linkedSourcesForActiveFile(state: CrivState): Promise<LinkedSource[]> {
    const file = this.app.workspace.getActiveFile();
    if (!file) {
      return [];
    }
    const markdown = await this.app.vault.cachedRead(file);
    return linkedSourcesFromMarkdown(markdown, state);
  }

  async decorateLinks(el: HTMLElement, _ctx: MarkdownPostProcessorContext) {
    const state = await this.getState();
    if (!state) {
      return;
    }
    const candidates = Array.from(
      el.querySelectorAll("[data-href], a.internal-link, a[href]"),
    ) as HTMLElement[];
    for (const anchor of candidates) {
      const source = resolveSourceFromElement(state, anchor);
      const pattern = resolvePatternFromElement(state, anchor);
      if (source) {
        anchor.addClass("criv-source-ref");
        anchor.setAttribute("title", sourceTooltip(state, source.entry));
        continue;
      }
      if (pattern) {
        anchor.addClass("criv-pattern-ref");
        anchor.setAttribute("title", patternTooltip(state, pattern));
        continue;
      }
      const target = linkTargets(anchor)[0] ?? "";
      if (looksLikeSourceOrPattern(target)) {
        anchor.addClass("criv-warning");
        anchor.setAttribute("title", "Unresolved criv reference");
      }
    }
  }

  async sourcePreview(linked: LinkedSource): Promise<SourcePreview> {
    const sourcePath = safeVaultPath(linked.entry.path);
    if (!sourcePath) {
      throw new Error(`Invalid source path ${linked.entry.path}`);
    }
    const raw = await this.app.vault.adapter.read(sourcePath);
    const lines = raw.split(/\r?\n/);
    const lineRange = parseLineRange(linked.fragment);
    const start = lineRange?.start ?? 1;
    const end = lineRange?.end ?? Math.min(lines.length, start + PREVIEW_LINE_LIMIT - 1);
    const selected = lines.slice(Math.max(0, start - 1), Math.min(lines.length, end));
    const truncated = !lineRange && start + selected.length - 1 < lines.length;
    return {
      path: sourcePath,
      language: languageForPath(sourcePath),
      text: selected.join("\n"),
      startLine: start,
      truncated,
    };
  }

  openExternal(path: string) {
    const url = this.settings.externalEditorUrl.replace("{path}", encodeURI(path));
    window.open(url);
  }

  async sourceEntries(): Promise<SourceIndexEntry[]> {
    const state = await this.getState();
    return sourceEntries(state);
  }

  async patternIds(): Promise<string[]> {
    const state = await this.getState();
    return state?.["registered-patterns"] ?? Object.keys(state?.patterns ?? {});
  }

  private async handleDocumentMouseOver(event: MouseEvent): Promise<void> {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const link = target?.closest(LINK_TARGET_SELECTOR) as HTMLElement | null;
    if (!link || link.closest(".criv-hover-preview")) {
      return;
    }

    const state = await this.getState();
    if (!state) {
      return;
    }
    const source = resolveSourceFromElement(state, link);
    if (!source) {
      return;
    }

    link.addClass("criv-source-ref");
    link.setAttribute("title", sourceTooltip(state, source.entry));
    await this.showHoverPreview(event, source);
  }

  private handleDocumentMouseOut(event: MouseEvent): void {
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
      const data = await this.sourcePreview(source);
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

  async saveSettings() {
    await this.saveData(this.settings);
  }

  private safeStatePath(): string | null {
    return safeVaultPath(this.settings.statePath);
  }

  private patchNativeSaveCommand(): void {
    const saveCommand = obsidianCommands(this.app)?.commands?.["editor:save-file"];
    if (!saveCommand) {
      return;
    }
    const originalCheckCallback = saveCommand.checkCallback;
    saveCommand.checkCallback = (checking: boolean) => {
      const c4View = this.app.workspace.getActiveViewOfType(CrivC4View);
      if (c4View?.canSaveSourceFromShortcut()) {
        if (!checking) {
          void c4View.saveSourceFromShortcut();
        }
        return true;
      }
      return originalCheckCallback?.(checking);
    };
    this.register(() => {
      saveCommand.checkCallback = originalCheckCallback;
    });
  }
}

function obsidianCommands(app: CrivPlugin["app"]): ObsidianCommandRegistry | null {
  return (app as unknown as { commands?: ObsidianCommandRegistry }).commands ?? null;
}

class CrivSourceView extends ItemView {
  constructor(
    leaf: WorkspaceLeaf,
    private plugin: CrivPlugin,
  ) {
    super(leaf);
  }

  getViewType(): string {
    return VIEW_TYPE;
  }

  getDisplayText(): string {
    return "criv";
  }

  async onOpen() {
    await this.render();
  }

  async render() {
    const container = this.containerEl.children[1] as HTMLElement;
    container.empty();
    container.addClass("criv-panel");
    const state = await this.plugin.getState();
    if (!state) {
      container.createEl("p", { cls: "criv-empty", text: this.plugin.stateStatus() });
      return;
    }

    const linkedSources = await this.plugin.linkedSourcesForActiveFile(state);
    const header = container.createDiv({ cls: "criv-panel-header" });
    header.createEl("h3", { text: "Linked source files" });
    header.createSpan({ text: `${linkedSources.length}` });

    if (linkedSources.length === 0) {
      container.createEl("p", {
        cls: "criv-empty",
        text: "No source links in the active note.",
      });
    }

    for (const source of linkedSources) {
      await this.renderLinkedSource(container, source);
    }

    const frontmatterPatterns = this.plugin.frontmatterPatternTargets(state);
    container.createEl("h3", { cls: "criv-section-title", text: "Pattern targets" });
    if (frontmatterPatterns.length === 0) {
      container.createEl("p", { cls: "criv-empty", text: "No frontmatter pattern targets." });
    }
    for (const target of frontmatterPatterns) {
      const row = container.createDiv({ cls: "criv-pattern-row" });
      row.addClass(target.status === "unresolved" ? "criv-warning" : "criv-pattern-ref");
      row.createSpan({ text: `${target.source}: ${target.id}` });
      row.createSpan({
        text:
          target.status === "local"
            ? "local target"
            : target.status === "resolved"
              ? `${target.matches.length} match${target.matches.length === 1 ? "" : "es"}`
              : "unresolved",
      });
      this.renderPatternMatches(container, target);
    }
  }

  private renderPatternMatches(container: HTMLElement, target: FrontmatterPatternTarget): void {
    if (target.status === "local" || target.status === "unresolved") {
      return;
    }
    const list = container.createDiv({ cls: "criv-pattern-match-list" });
    if (target.matches.length === 0) {
      list.createDiv({ cls: "criv-pattern-match-empty", text: "No matches" });
      return;
    }
    for (const match of target.matches) {
      const item = list.createDiv({ cls: "criv-pattern-match" });
      const head = item.createDiv({ cls: "criv-pattern-match-head" });
      head.createSpan({ text: match.file });
      head.createSpan({ text: match.range ?? "range unavailable" });
      const captures = Object.entries(match.captures);
      if (captures.length > 0) {
        const captureList = item.createDiv({ cls: "criv-pattern-captures" });
        for (const [name, value] of captures) {
          const capture = captureList.createDiv({ cls: "criv-pattern-capture" });
          capture.createSpan({ text: `$${name}` });
          capture.createEl("code", { text: value });
        }
      }
    }
  }

  private async renderLinkedSource(container: HTMLElement, source: LinkedSource): Promise<void> {
    const card = container.createDiv({ cls: "criv-source-card" });
    const head = card.createDiv({ cls: "criv-source-card-head" });
    const openButton = head.createEl("button", { text: source.entry.path });
    openButton.onclick = () => this.plugin.openExternal(source.entry.path);
    head.createSpan({ text: source.entry.mime ?? languageForPath(source.entry.path) });

    try {
      const preview = await this.plugin.sourcePreview(source);
      renderPreview(card, preview, true);
    } catch {
      renderPreviewError(card, source.entry.path);
    }
  }
}

class CrivC4View extends FileView {
  private source = "";
  private draftSource = "";
  private sourcePath: string | null = null;
  private mode: "preview" | "source" = "preview";
  private scale = 1;
  private panX = 0;
  private panY = 0;
  private transformInitialized = false;
  private dirtyBadgeEl: HTMLElement | null = null;
  private sourceEditorEl: HTMLTextAreaElement | null = null;
  private sourceSaveHandlerRegistered = false;

  constructor(leaf: WorkspaceLeaf) {
    super(leaf);
    this.registerSourceSaveHandler();
  }

  getViewType(): string {
    return C4_VIEW_TYPE;
  }

  getDisplayText(): string {
    return this.file?.basename ?? "C4";
  }

  async onOpen(): Promise<void> {
    this.registerSourceSaveHandler();
    await this.render();
  }

  async onLoadFile(file: TFile): Promise<void> {
    this.source = await this.app.vault.cachedRead(file);
    this.draftSource = this.source;
    this.sourcePath = file.path;
    this.mode = "preview";
    this.sourceEditorEl = null;
    this.resetTransformState();
    await this.render();
  }

  async onUnloadFile(file: TFile): Promise<void> {
    if (this.sourcePath === file.path) {
      this.source = "";
      this.draftSource = "";
      this.sourcePath = null;
      this.sourceEditorEl = null;
      this.resetTransformState();
    }
    await this.render();
  }

  async render(): Promise<void> {
    this.registerSourceSaveHandler();
    const container = this.containerEl.children[1] as HTMLElement;
    container.empty();
    container.addClass("criv-c4-view");
    this.dirtyBadgeEl = null;
    this.sourceEditorEl = null;

    if (!this.file) {
      container.createEl("p", { cls: "criv-empty", text: "No C4 file selected." });
      return;
    }

    await this.sourceForCurrentFile();
    const source = this.currentSource();
    const summary = parseC4Artifact(this.file.path, source);
    const header = container.createDiv({ cls: "criv-c4-header" });
    header.createEl("h3", { text: this.file.basename });
    const meta = header.createDiv({ cls: "criv-c4-meta" });
    meta.createSpan({ text: summary.format });
    meta.createSpan({ text: summary.level });
    if (summary.generated) {
      meta.createSpan({ text: "generated" });
    }
    this.dirtyBadgeEl = meta.createSpan({ cls: "criv-warning criv-c4-dirty", text: "unsaved" });
    this.updateDirtyBadge();
    if (summary.diagnostics.length > 0) {
      meta.createSpan({ cls: "criv-warning", text: `${summary.diagnostics.length}` });
    }
    const toolbar = header.createDiv({ cls: "criv-c4-toolbar" });
    this.renderToolbar(toolbar);

    const body = container.createDiv({ cls: "criv-c4-body" });
    if (this.mode === "source") {
      this.renderSourceEditor(body);
    } else {
      await this.renderPreview(body, summary, source);
    }

    if (summary.diagnostics.length > 0) {
      const diagnostics = container.createDiv({ cls: "criv-c4-diagnostics" });
      for (const diagnostic of summary.diagnostics) {
        const row = diagnostics.createDiv({ cls: "criv-c4-diagnostic" });
        row.createSpan({ text: diagnostic.line ? `L${diagnostic.line}` : "--" });
        row.createSpan({ text: diagnostic.code });
        row.createSpan({ text: diagnostic.message });
      }
    }
  }

  private async sourceForCurrentFile(): Promise<string> {
    if (!this.file) {
      return "";
    }
    if (this.sourcePath !== this.file.path) {
      this.source = await this.app.vault.cachedRead(this.file);
      this.draftSource = this.source;
      this.sourcePath = this.file.path;
      this.mode = "preview";
      this.sourceEditorEl = null;
      this.resetTransformState();
    }
    return this.source;
  }

  private currentSource(): string {
    return this.sourceDirty() ? this.draftSource : this.source;
  }

  private sourceDirty(): boolean {
    return this.draftSource !== this.source;
  }

  canSaveSourceFromShortcut(): boolean {
    return this.mode === "source" && this.sourceEditorEl !== null;
  }

  async saveSourceFromShortcut(): Promise<void> {
    if (!this.sourceEditorEl) {
      return;
    }
    await this.saveSource(this.sourceEditorEl.value);
  }

  private renderToolbar(toolbar: HTMLElement): void {
    this.toolbarButton(toolbar, "Preview", "Preview diagram", this.mode === "preview", () => {
      this.mode = "preview";
      void this.render();
    });
    this.toolbarButton(toolbar, "Source", "Edit source", this.mode === "source", () => {
      this.mode = "source";
      void this.render();
    });
    if (this.mode !== "preview") {
      return;
    }
    this.toolbarButton(toolbar, "+", "Zoom in", false, () => this.zoomBy(1.2));
    this.toolbarButton(toolbar, "-", "Zoom out", false, () => this.zoomBy(1 / 1.2));
    this.toolbarButton(toolbar, "Fit", "Fit diagram", false, () => this.fitPreview("contain"));
    this.toolbarButton(toolbar, "Reset", "Reset view", false, () => {
      this.scale = 1;
      this.panX = 0;
      this.panY = 0;
      this.transformInitialized = true;
      this.applyPreviewTransform();
    });
  }

  private toolbarButton(
    toolbar: HTMLElement,
    text: string,
    tooltip: string,
    active: boolean,
    onClick: () => void,
  ): void {
    const button = toolbar.createEl("button", { text, attr: { "aria-label": tooltip } });
    button.setAttribute("title", tooltip);
    if (active) {
      button.addClass("is-active");
    }
    button.onclick = onClick;
  }

  private async renderPreview(
    body: HTMLElement,
    summary: C4ArtifactSummary,
    source: string,
  ): Promise<void> {
    const viewport = body.createDiv({ cls: "criv-c4-preview" });
    const surface = viewport.createDiv({ cls: "criv-c4-preview-surface" });
    await renderC4Projection(surface, summary, source);
    normalizePreviewSvg(surface);
    this.bindPreviewInput(viewport);
    this.applyPreviewTransform();
    if (!this.transformInitialized) {
      requestAnimationFrame(() => this.fitPreview("width"));
    }
  }

  private renderSourceEditor(body: HTMLElement): void {
    const sourcePanel = body.createDiv({ cls: "criv-c4-source" });
    const editor = sourcePanel.createEl("textarea", { cls: "criv-c4-editor" });
    this.sourceEditorEl = editor;
    editor.value = this.draftSource;
    editor.spellcheck = false;
    editor.oninput = () => {
      this.draftSource = editor.value;
      this.updateDirtyBadge();
    };
    const saveOnModS = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        event.stopPropagation();
        void this.saveSource(editor.value);
      }
    };
    editor.addEventListener("keydown", saveOnModS, { capture: true });
    editor.focus();
  }

  private async saveSource(value: string): Promise<void> {
    if (!this.file) {
      return;
    }
    this.draftSource = value;
    await this.app.vault.modify(this.file, value);
    this.source = value;
    this.sourcePath = this.file.path;
    this.transformInitialized = false;
    await this.render();
  }

  private bindPreviewInput(viewport: HTMLElement): void {
    viewport.onwheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = viewport.getBoundingClientRect();
      const factor = event.deltaY < 0 ? 1.1 : 1 / 1.1;
      this.zoomAt(factor, event.clientX - rect.left, event.clientY - rect.top);
    };

    viewport.onpointerdown = (event: PointerEvent) => {
      if (event.button !== 0) {
        return;
      }
      event.preventDefault();
      viewport.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const startY = event.clientY;
      const initialX = this.panX;
      const initialY = this.panY;

      viewport.onpointermove = (moveEvent: PointerEvent) => {
        this.panX = initialX + moveEvent.clientX - startX;
        this.panY = initialY + moveEvent.clientY - startY;
        this.transformInitialized = true;
        this.applyPreviewTransform();
      };
      const stopPan = () => {
        viewport.onpointermove = null;
        viewport.onpointerup = null;
        viewport.onpointercancel = null;
      };
      viewport.onpointerup = stopPan;
      viewport.onpointercancel = stopPan;
    };
  }

  private zoomBy(factor: number): void {
    const viewport = this.containerEl.querySelector(".criv-c4-preview") as HTMLElement | null;
    if (!viewport) {
      return;
    }
    this.zoomAt(factor, viewport.clientWidth / 2, viewport.clientHeight / 2);
  }

  private zoomAt(factor: number, originX: number, originY: number): void {
    const nextScale = clamp(this.scale * factor, 0.05, 8);
    const ratio = nextScale / this.scale;
    this.panX = originX - (originX - this.panX) * ratio;
    this.panY = originY - (originY - this.panY) * ratio;
    this.scale = nextScale;
    this.transformInitialized = true;
    this.applyPreviewTransform();
  }

  private fitPreview(mode: "contain" | "width"): void {
    const viewport = this.containerEl.querySelector(".criv-c4-preview") as HTMLElement | null;
    const surface = this.containerEl.querySelector(
      ".criv-c4-preview-surface",
    ) as HTMLElement | null;
    const svg = surface?.querySelector("svg") as SVGSVGElement | null;
    if (!viewport || !surface || !svg) {
      return;
    }
    const size = svgSize(svg);
    if (!size) {
      return;
    }
    const padding = 32;
    const availableWidth = Math.max(1, viewport.clientWidth - padding * 2);
    const availableHeight = Math.max(1, viewport.clientHeight - padding * 2);
    const widthScale = availableWidth / size.width;
    const containScale = Math.min(widthScale, availableHeight / size.height);
    this.scale = mode === "width" ? clamp(widthScale, 0.35, 1.25) : clamp(containScale, 0.05, 2);
    this.panX =
      mode === "width" && size.width * this.scale > availableWidth
        ? padding
        : (viewport.clientWidth - size.width * this.scale) / 2;
    this.panY = mode === "width" ? padding : (viewport.clientHeight - size.height * this.scale) / 2;
    this.transformInitialized = true;
    this.applyPreviewTransform();
  }

  private applyPreviewTransform(): void {
    const surface = this.containerEl.querySelector(
      ".criv-c4-preview-surface",
    ) as HTMLElement | null;
    if (!surface) {
      return;
    }
    surface.style.transform = `translate(${this.panX}px, ${this.panY}px) scale(${this.scale})`;
  }

  private resetTransformState(): void {
    this.scale = 1;
    this.panX = 0;
    this.panY = 0;
    this.transformInitialized = false;
  }

  private updateDirtyBadge(): void {
    this.dirtyBadgeEl?.toggleClass("is-hidden", !this.sourceDirty());
  }

  private registerSourceSaveHandler(): void {
    if (this.sourceSaveHandlerRegistered || !this.scope) {
      return;
    }
    this.scope.register(["Mod"], "s", () => {
      if (this.mode !== "source" || !this.sourceEditorEl) {
        return;
      }
      void this.saveSource(this.sourceEditorEl.value);
      return false;
    });
    this.sourceSaveHandlerRegistered = true;
  }
}

interface C4RendererAdapter {
  render(container: HTMLElement, summary: C4ArtifactSummary, source: string): Promise<void>;
}

const sourceProjectionRenderer: C4RendererAdapter = {
  async render(container, summary, source) {
    container.empty();
    container.addClass(`criv-c4-projection-${safeCssSegment(summary.format)}`);
    const preview = container.createEl("pre");
    preview.createEl("code", {
      cls: `language-${summary.format === "dot" ? "dot" : "mermaid"}`,
      text: source,
    });
  },
};

const mermaidRenderer: C4RendererAdapter = {
  async render(container, summary, source) {
    container.empty();
    container.addClass("criv-c4-projection-mermaid");
    const renderId = `criv-c4-${safeCssSegment(summary.level)}-${++c4RenderSequence}`;
    mermaid.initialize({
      securityLevel: "strict",
      startOnLoad: false,
      theme: "base",
    });
    try {
      const result = await mermaid.render(renderId, source);
      container.innerHTML = result.svg;
    } catch (error) {
      await sourceProjectionRenderer.render(container, summary, source);
      container.createDiv({
        cls: "criv-c4-render-error",
        text: `Could not render Mermaid preview: ${errorMessage(error)}`,
      });
    }
  },
};

const dotRenderer: C4RendererAdapter = {
  async render(container, summary, source) {
    container.empty();
    container.addClass("criv-c4-projection-dot");
    try {
      const viz = await c4DotRendererInstance();
      const result = viz.render(source, { engine: "dot", format: "svg" });
      if (result.status === "failure") {
        throw new Error(renderErrorsMessage(result.errors));
      }
      container.innerHTML = sanitizeDotSvg(result.output);
      renderWarnings(container, result.errors);
    } catch (error) {
      await sourceProjectionRenderer.render(container, summary, source);
      container.createDiv({
        cls: "criv-c4-render-error",
        text: `Could not render DOT preview: ${errorMessage(error)}`,
      });
    }
  },
};

async function renderC4Projection(
  container: HTMLElement,
  summary: C4ArtifactSummary,
  source: string,
): Promise<void> {
  if (summary.format === "mermaid") {
    await mermaidRenderer.render(container, summary, source);
    return;
  }
  if (summary.format === "dot") {
    await dotRenderer.render(container, summary, source);
    return;
  }
  await sourceProjectionRenderer.render(container, summary, source);
}

function c4DotRendererInstance(): Promise<Viz> {
  c4DotRenderer ??= vizInstance();
  return c4DotRenderer;
}

function renderWarnings(
  container: HTMLElement,
  errors: { level?: string; message: string }[],
): void {
  const warnings = errors.filter((error) => error.level === "warning");
  if (warnings.length === 0) {
    return;
  }
  const warningList = container.createDiv({ cls: "criv-c4-render-warning" });
  for (const warning of warnings) {
    warningList.createDiv({ text: warning.message });
  }
}

class CrivSourceSuggest extends EditorSuggest<SourceSuggestionItem> {
  constructor(private plugin: CrivPlugin) {
    super(plugin.app);
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
    const state = await this.plugin.getState();
    if (!state) {
      return [];
    }
    // The wasm API accepts raw state JSON; the plugin currently caches the parsed state only.
    const wasmSuggestions = await suggestSourceSelectors(JSON.stringify(state), context.query, 20);
    if (wasmSuggestions) {
      return sourceSuggestionItemsFromWasm(wasmSuggestions);
    }
    return sourceSuggestions(state, context.query, 20).map((entry) => ({
      insertText: entry.path,
      label: entry.path,
      path: entry.path,
      detail: entry.mime,
    }));
  }

  renderSuggestion(value: SourceSuggestionItem, el: HTMLElement): void {
    el.createDiv({ text: value.label });
    if (value.detail) {
      el.createDiv({ cls: "criv-source-suggestion-detail", text: value.detail });
    }
  }

  selectSuggestion(value: SourceSuggestionItem): void {
    if (!this.context) {
      return;
    }
    this.context.editor.replaceRange(value.insertText, this.context.start, this.context.end);
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

function resolveSourceFromElement(state: CrivState, element: HTMLElement): LinkedSource | null {
  for (const target of linkTargets(element)) {
    const source = resolveSource(state, target);
    if (source) {
      return source;
    }
  }
  return null;
}

function resolvePatternFromElement(state: CrivState, element: HTMLElement): string | null {
  for (const target of linkTargets(element)) {
    const pattern = resolvePattern(state, target);
    if (pattern) {
      return pattern;
    }
  }
  return null;
}

function linkTargets(element: HTMLElement): string[] {
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
    addTarget(targets, decodeURIComponent(href.replace(/^#/, "")));
  }

  addTextTargets(targets, element.textContent);
  addTextTargets(targets, (element.closest(".cm-line") as HTMLElement | null)?.textContent);
  return Array.from(new Set(targets));
}

function renderPreview(container: HTMLElement, preview: SourcePreview, compact: boolean): void {
  container.querySelector(".criv-preview-loading")?.remove();
  container.querySelector(".criv-preview-error")?.remove();
  const existing = container.querySelector(".criv-preview-body");
  existing?.remove();

  const body = container.createDiv({ cls: "criv-preview-body" });
  if (!compact) {
    body.createDiv({ cls: "criv-preview-path", text: preview.path });
  }
  const meta = body.createDiv({ cls: "criv-preview-meta" });
  meta.createSpan({ text: preview.language || "text" });
  meta.createSpan({ text: `L${preview.startLine}` });
  if (preview.truncated) {
    meta.createSpan({ text: "truncated" });
  }
  const source = body.createDiv({ cls: "criv-source-preview" });
  source.createEl("pre", {
    cls: "criv-source-lines",
    text: lineNumbers(preview.text, preview.startLine),
  });
  renderHighlightedCode(source, preview);
}

function renderPreviewError(container: HTMLElement, path: string): void {
  container.querySelector(".criv-preview-loading")?.remove();
  const existing = container.querySelector(".criv-preview-body");
  existing?.remove();
  container.createDiv({
    cls: "criv-preview-error",
    text: `Could not read ${path}`,
  });
}

function positionHoverPreview(preview: HTMLElement, event: MouseEvent): void {
  const margin = 16;
  const width = Math.min(560, window.innerWidth - margin * 2);
  preview.style.width = `${width}px`;
  preview.style.left = `${Math.min(event.clientX + margin, window.innerWidth - width - margin)}px`;
  preview.style.top = `${Math.min(event.clientY + margin, window.innerHeight - 260)}px`;
}

function lineNumbers(text: string, startLine: number): string {
  return text
    .split("\n")
    .map((_line, index) => String(startLine + index).padStart(4, " "))
    .join("\n");
}

function languageForPath(path: string): string {
  const extension = path.split(".").pop()?.toLowerCase();
  switch (extension) {
    case "rs":
      return "rust";
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
      return "javascript";
    case "py":
      return "python";
    case "go":
      return "go";
    default:
      return extension ?? "text";
  }
}

interface HighlightToken {
  text: string;
  className?: string;
}

function renderHighlightedCode(container: HTMLElement, preview: SourcePreview): void {
  const pre = container.createEl("pre", {
    cls: "criv-source-code criv-source-code-highlighted",
  });
  const code = pre.createEl("code", {
    cls: `language-${safeCssSegment(preview.language)}`,
  });
  const lines = preview.text.split("\n");
  lines.forEach((line, lineIndex) => {
    for (const token of highlightLine(line, preview.language)) {
      if (token.className) {
        code.createSpan({ cls: token.className, text: token.text });
      } else {
        code.appendText(token.text);
      }
    }
    if (lineIndex + 1 < lines.length) {
      code.appendText("\n");
    }
  });
}

function highlightLine(line: string, language: string): HighlightToken[] {
  const tokens: HighlightToken[] = [];
  const tokenPattern =
    language === "python"
      ? /#.*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b/g
      : /\/\/.*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b/g;
  let cursor = 0;
  for (const match of line.matchAll(tokenPattern)) {
    const index = match.index ?? cursor;
    if (index > cursor) {
      tokens.push({ text: line.slice(cursor, index) });
    }
    const text = match[0];
    tokens.push({ text, className: highlightClass(text, language) });
    cursor = index + text.length;
  }
  if (cursor < line.length) {
    tokens.push({ text: line.slice(cursor) });
  }
  return tokens;
}

function highlightClass(token: string, language: string): string | undefined {
  if (token.startsWith("//") || (language === "python" && token.startsWith("#"))) {
    return "criv-token-comment";
  }
  if (token.startsWith('"') || token.startsWith("'") || token.startsWith("`")) {
    return "criv-token-string";
  }
  if (/^\d/.test(token)) {
    return "criv-token-number";
  }
  if (keywordSet(language).has(token)) {
    return "criv-token-keyword";
  }
  if (literalSet(language).has(token)) {
    return "criv-token-literal";
  }
  if (/^[A-Z][A-Za-z0-9_]*$/.test(token)) {
    return "criv-token-type";
  }
  return undefined;
}

function keywordSet(language: string): Set<string> {
  switch (language) {
    case "rust":
      return new Set([
        "as",
        "async",
        "await",
        "const",
        "crate",
        "enum",
        "fn",
        "for",
        "if",
        "impl",
        "let",
        "match",
        "mod",
        "mut",
        "pub",
        "return",
        "self",
        "static",
        "struct",
        "trait",
        "type",
        "use",
        "where",
        "while",
      ]);
    case "typescript":
    case "javascript":
      return new Set([
        "async",
        "await",
        "class",
        "const",
        "else",
        "export",
        "for",
        "from",
        "function",
        "if",
        "import",
        "interface",
        "let",
        "new",
        "private",
        "return",
        "type",
      ]);
    case "python":
      return new Set([
        "as",
        "async",
        "await",
        "class",
        "def",
        "elif",
        "else",
        "for",
        "from",
        "if",
        "import",
        "in",
        "lambda",
        "return",
        "self",
        "while",
      ]);
    case "go":
      return new Set([
        "const",
        "defer",
        "else",
        "for",
        "func",
        "go",
        "if",
        "import",
        "interface",
        "package",
        "range",
        "return",
        "struct",
        "type",
        "var",
      ]);
    default:
      return new Set();
  }
}

function literalSet(language: string): Set<string> {
  if (language === "python") {
    return new Set(["False", "None", "True"]);
  }
  return new Set(["false", "null", "true", "undefined"]);
}

function safeCssSegment(value: string): string {
  return /^[a-z0-9_-]+$/i.test(value) ? value : "text";
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function svgSize(svg: SVGSVGElement): { width: number; height: number } | null {
  const viewBox = svg.viewBox.baseVal;
  if (viewBox.width > 0 && viewBox.height > 0) {
    return { width: viewBox.width, height: viewBox.height };
  }
  const width = parseSvgLength(svg.getAttribute("width"));
  const height = parseSvgLength(svg.getAttribute("height"));
  if (width && height) {
    return { width, height };
  }
  const rect = svg.getBoundingClientRect();
  if (rect.width > 0 && rect.height > 0) {
    return { width: rect.width, height: rect.height };
  }
  return null;
}

function parseSvgLength(value: string | null): number | null {
  const match = value?.match(/^([\d.]+)/);
  if (!match) {
    return null;
  }
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function normalizePreviewSvg(surface: HTMLElement): void {
  const svg = surface.querySelector("svg") as SVGSVGElement | null;
  if (!svg) {
    return;
  }
  const size = svgSize(svg);
  if (!size) {
    return;
  }
  surface.style.width = `${size.width}px`;
  surface.style.height = `${size.height}px`;
  svg.style.width = `${size.width}px`;
  svg.style.height = `${size.height}px`;
  svg.removeAttribute("width");
  svg.removeAttribute("height");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

class CrivEditorDriftPlugin implements PluginValue {
  decorations: DecorationSet;

  constructor(
    view: EditorView,
    private plugin: CrivPlugin,
  ) {
    this.decorations = this.buildDecorations(view);
  }

  update(update: ViewUpdate): void {
    if (update.docChanged || update.viewportChanged) {
      this.decorations = this.buildDecorations(update.view);
    }
  }

  buildDecorations(view: EditorView): DecorationSet {
    const state = this.plugin.cachedState();
    if (!state) {
      return Decoration.none;
    }
    const builder = new RangeSetBuilder<Decoration>();
    for (const { from, to } of view.visibleRanges) {
      const text = view.state.sliceDoc(from, to);
      for (const range of crivLinkRanges(text, state)) {
        if (range.status !== "unresolved") {
          continue;
        }
        builder.add(
          from + range.from,
          from + range.to,
          Decoration.mark({
            class: "criv-editor-warning",
            attributes: {
              "data-criv-target": range.target,
              title: "Unresolved criv reference",
            },
          }),
        );
      }
    }
    return builder.finish();
  }
}

function crivDriftExtension(plugin: CrivPlugin) {
  return ViewPlugin.fromClass<CrivEditorDriftPlugin, CrivPlugin>(CrivEditorDriftPlugin, {
    decorations: (value) => value.decorations,
  }).of(plugin);
}

class CrivSettingTab extends PluginSettingTab {
  plugin: CrivPlugin;

  constructor(app: CrivPlugin["app"], plugin: CrivPlugin) {
    super(app, plugin);
    this.plugin = plugin;
  }

  display(): void {
    const { containerEl } = this;
    containerEl.empty();

    new Setting(containerEl)
      .setName("State path")
      .setDesc("Path to the criv watcher state file, relative to the vault root.")
      .addText((text) =>
        text
          .setPlaceholder(".criv/state.json")
          .setValue(this.plugin.settings.statePath)
          .onChange(async (value) => {
            this.plugin.settings.statePath = safeVaultPath(value) ?? DEFAULT_SETTINGS.statePath;
            await this.plugin.saveSettings();
          }),
      );

    new Setting(containerEl)
      .setName("External editor URL")
      .setDesc(
        "URL template for opening source files. Use {path} for the repo-relative source path.",
      )
      .addText((text) =>
        text
          .setPlaceholder("vscode://file/{path}")
          .setValue(this.plugin.settings.externalEditorUrl)
          .onChange(async (value) => {
            this.plugin.settings.externalEditorUrl =
              value.trim() || DEFAULT_SETTINGS.externalEditorUrl;
            await this.plugin.saveSettings();
          }),
      );
  }
}

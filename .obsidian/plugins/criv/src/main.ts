import {
  Editor,
  EditorPosition,
  EditorSuggest,
  EditorSuggestContext,
  EditorSuggestTriggerInfo,
  ItemView,
  MarkdownPostProcessorContext,
  Notice,
  Plugin,
  PluginSettingTab,
  Setting,
  WorkspaceLeaf,
} from "obsidian";
import { summarizeState } from "./wasm";

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
const PREVIEW_LINE_LIMIT = 80;
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

interface CrivNode {
  id: string;
  kind: string;
  label: string;
  path?: string;
}

interface PatternMatch {
  file: string;
  range?: string;
  captures: Record<string, string>;
}

interface SourceIndexEntry {
  path: string;
  frecency: number;
  mime?: string;
}

interface LinkedSource {
  target: string;
  fragment: string | null;
  entry: SourceIndexEntry;
}

interface SourcePreview {
  path: string;
  language: string;
  text: string;
  startLine: number;
  truncated: boolean;
}

interface CrivState {
  schema: string;
  graph?: { nodes?: CrivNode[] };
  patterns?: Record<string, PatternMatch[]>;
  "registered-patterns"?: string[];
  "source-index"?: SourceIndexEntry[];
}

interface FrontmatterPatternTarget {
  id: string;
  source: "targets" | "policy";
  status: "resolved" | "local" | "unresolved";
  matches: number;
}

export default class CrivPlugin extends Plugin {
  settings: CrivSettings;
  private state: CrivState | null = null;
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
    this.registerView(VIEW_TYPE, (leaf) => new CrivSourceView(leaf, this));
    this.registerMarkdownPostProcessor((el, ctx) => this.decorateLinks(el, ctx));
    this.registerDomEvent(document, "mouseover", (event) => this.handleDocumentMouseOver(event));
    this.registerDomEvent(document, "mouseout", (event) => this.handleDocumentMouseOut(event));
    this.registerEditorSuggest(new CrivSourceSuggest(this));
    this.registerEvent(this.app.workspace.on("active-leaf-change", () => this.refreshSourcePanel()));
    this.registerEvent(this.app.metadataCache.on("changed", () => this.refreshSourcePanel()));
    this.addSettingTab(new CrivSettingTab(this.app, this));
    this.app.workspace.onLayoutReady(() => {
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
    const existing = this.app.workspace.getLeavesOfType(VIEW_TYPE)[0];
    if (existing) {
      if (reveal) {
        this.app.workspace.revealLeaf(existing);
      }
      await this.refreshSourcePanel();
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
    try {
      const raw = await this.app.vault.adapter.read(this.settings.statePath);
      return await summarizeState(raw);
    } catch {
      return null;
    }
  }

  async loadState(): Promise<CrivState | null> {
    try {
      const raw = await this.app.vault.adapter.read(this.settings.statePath);
      const state = JSON.parse(raw) as CrivState;
      if (state.schema !== EXPECTED_SCHEMA) {
        return null;
      }
      this.state = state;
      return state;
    } catch {
      return null;
    }
  }

  async getState(): Promise<CrivState | null> {
    return this.state ?? (await this.loadState());
  }

  async refreshSourcePanel(): Promise<void> {
    for (const leaf of this.app.workspace.getLeavesOfType(VIEW_TYPE)) {
      if (leaf.view instanceof CrivSourceView) {
        await leaf.view.render();
      }
    }
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
    const raw = await this.app.vault.adapter.read(linked.entry.path);
    const lines = raw.split(/\r?\n/);
    const lineRange = parseLineRange(linked.fragment);
    const start = lineRange?.start ?? 1;
    const end = lineRange?.end ?? Math.min(lines.length, start + PREVIEW_LINE_LIMIT - 1);
    const selected = lines.slice(Math.max(0, start - 1), Math.min(lines.length, end));
    const truncated = !lineRange && start + selected.length - 1 < lines.length;
    return {
      path: linked.entry.path,
      language: languageForPath(linked.entry.path),
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
    return state?.["source-index"] ?? [];
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
}

class CrivSourceView extends ItemView {
  constructor(leaf: WorkspaceLeaf, private plugin: CrivPlugin) {
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
      container.createEl("p", { cls: "criv-empty", text: "criv state is unavailable." });
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
              ? `${target.matches} match${target.matches === 1 ? "" : "es"}`
              : "unresolved",
      });
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

class CrivSourceSuggest extends EditorSuggest<SourceIndexEntry> {
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

  async getSuggestions(context: EditorSuggestContext): Promise<SourceIndexEntry[]> {
    const query = context.query.toLowerCase();
    return (await this.plugin.sourceEntries())
      .filter((entry) => entry.path.toLowerCase().includes(query))
      .slice(0, 20);
  }

  renderSuggestion(value: SourceIndexEntry, el: HTMLElement): void {
    el.createDiv({ text: value.path });
  }

  selectSuggestion(value: SourceIndexEntry): void {
    if (!this.context) {
      return;
    }
    this.context.editor.replaceRange(value.path, this.context.start, this.context.end);
  }
}

function linkedSourcesFromMarkdown(markdown: string, state: CrivState): LinkedSource[] {
  const links = Array.from(markdown.matchAll(/\[\[([^\]]+)\]\]/g))
    .map((match) => match[1] ?? "")
    .map((target) => resolveSource(state, target))
    .filter((source): source is LinkedSource => source !== null);
  const seen = new Set<string>();
  return links.filter((source) => {
    const key = `${source.entry.path}#${source.fragment ?? ""}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
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
  addTarget(targets, element.getAttribute("data-href"));

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

function addTextTargets(targets: string[], value: string | null | undefined): void {
  if (!value) {
    return;
  }
  addTarget(targets, value);
  for (const match of value.matchAll(/\[\[([^\]]+)\]\]/g)) {
    addTarget(targets, match[1]);
  }
  const stripped = value.replace(/^\[\[/, "").replace(/\]\]$/, "");
  if (stripped !== value) {
    addTarget(targets, stripped);
  }
}

function addTarget(targets: string[], value: string | null | undefined): void {
  const target = value?.trim();
  if (target) {
    targets.push(target);
  }
}

function resolveSource(state: CrivState, target: string): LinkedSource | null {
  const clean = cleanTarget(target);
  const normalized = clean.split("#")[0] ?? "";
  if (!normalized || normalized.startsWith("match:")) {
    return null;
  }
  const entries = state["source-index"] ?? [];
  const entry =
    entries.find((candidate) => candidate.path === normalized) ??
    entries.find(
      (candidate) => candidate.path.endsWith(normalized) || candidate.path.split("/").pop() === normalized,
    );
  if (!entry) {
    return null;
  }
  return {
    target,
    fragment: clean.includes("#") ? clean.split("#").slice(1).join("#") : null,
    entry,
  };
}

function resolvePattern(state: CrivState, target: string): string | null {
  const clean = cleanTarget(target);
  const id = clean.startsWith("match:") ? clean.slice("match:".length) : clean.split("#match:")[1];
  if (!id) {
    return null;
  }
  const ids = state["registered-patterns"] ?? Object.keys(state.patterns ?? {});
  return ids.includes(id) ? id : null;
}

function cleanTarget(target: string): string {
  return target.split("|")[0]?.trim() ?? "";
}

function sourceTooltip(state: CrivState, source: SourceIndexEntry): string {
  const node = state.graph?.nodes?.find((candidate) => candidate.path === source.path);
  return node ? `${node.kind}: ${node.label}` : source.path;
}

function patternTooltip(state: CrivState, id: string): string {
  const count = state.patterns?.[id]?.length ?? 0;
  return `${id}: ${count} match${count === 1 ? "" : "es"}`;
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
  body.createEl("pre", {
    cls: "criv-source-preview",
    text: withLineNumbers(preview.text, preview.startLine),
  });
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

function parseLineRange(fragment: string | null): { start: number; end: number } | null {
  const match = fragment?.match(/^L(\d+)(?:-L?(\d+))?$/i);
  if (!match) {
    return null;
  }
  const start = Number(match[1]);
  const end = Number(match[2] ?? match[1]);
  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return null;
  }
  return { start: Math.max(1, start), end: Math.max(start, end) };
}

function withLineNumbers(text: string, startLine: number): string {
  return text
    .split("\n")
    .map((line, index) => `${String(startLine + index).padStart(4, " ")}  ${line}`)
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

function frontmatterPatternTargets(
  frontmatter: Record<string, unknown> | undefined,
  state: CrivState,
): FrontmatterPatternTarget[] {
  const targets: FrontmatterPatternTarget[] = [];
  const noteId = stringValue(frontmatter?.id);
  const targetObject = objectValue(frontmatter?.targets);
  for (const pattern of patternList(targetObject?.patterns)) {
    const target = frontmatterPatternTarget(pattern, "targets", noteId, state);
    if (target) {
      targets.push(target);
    }
  }

  const policyObject = objectValue(frontmatter?.policy);
  for (const pattern of patternList(policyObject?.patterns)) {
    const target = frontmatterPatternTarget(pattern, "policy", noteId, state);
    if (target) {
      targets.push(target);
    }
  }
  return targets;
}

function frontmatterPatternTarget(
  pattern: unknown,
  source: FrontmatterPatternTarget["source"],
  noteId: string | null,
  state: CrivState,
): FrontmatterPatternTarget | null {
  const object = objectValue(pattern);
  const rawRef = object ? stringValue(object.ref) : null;
  const rawId = object ? stringValue(object.id) : stringValue(pattern);
  const id = rawRef ?? (source === "policy" && rawId && noteId ? `${noteId}/${rawId}` : rawId);
  if (!id) {
    return null;
  }
  if (source === "targets" && !rawRef) {
    return { id, source, status: "local", matches: 0 };
  }

  const matches = state.patterns?.[id]?.length ?? 0;
  const ids = state["registered-patterns"] ?? Object.keys(state.patterns ?? {});
  return {
    id,
    source,
    status: ids.includes(id) || state.patterns?.[id] ? "resolved" : "unresolved",
    matches,
  };
}

function patternList(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value;
  }
  return value ? [value] : [];
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function looksLikeSourceOrPattern(target: string): boolean {
  const clean = cleanTarget(target);
  return clean.startsWith("match:") || /\.[a-z0-9]+(#.*)?$/i.test(clean);
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
            this.plugin.settings.statePath = value.trim() || DEFAULT_SETTINGS.statePath;
            await this.plugin.saveSettings();
          }),
      );

    new Setting(containerEl)
      .setName("External editor URL")
      .setDesc("URL template for opening source files. Use {path} for the repo-relative source path.")
      .addText((text) =>
        text
          .setPlaceholder("vscode://file/{path}")
          .setValue(this.plugin.settings.externalEditorUrl)
          .onChange(async (value) => {
            this.plugin.settings.externalEditorUrl = value.trim() || DEFAULT_SETTINGS.externalEditorUrl;
            await this.plugin.saveSettings();
          }),
      );
  }
}

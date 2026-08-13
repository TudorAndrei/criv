import { Plugin } from "obsidian";
import type { App, PluginManifest } from "obsidian";
import { C4_VIEW_TYPE, CrivC4View, patchNativeSaveCommand, shutdownC4Views } from "./c4-view";
import type { CrivState } from "./core";
import type { C4ViewPort, DisposableSubscription, ObsidianStateStatus } from "./ports";
import { ObsidianSettingsOwner, type CrivSettings } from "./settings";
import { ObsidianSourcePanelOwner, SOURCE_PANEL_VIEW_TYPE } from "./source-panel";
import { ObsidianSourceReferencesOwner } from "./source-references";
import { ObsidianStateOwner } from "./state-owner";
import { loadState as loadWasmState, type CrivLoadedState, type CrivStateSummary } from "./wasm";

const STATE_POLL_INTERVAL_MS = 2_000;

export { CrivC4View };
export type { ObsidianStateStatus } from "./ports";

export default class CrivPlugin extends Plugin {
  settings!: CrivSettings;
  private readonly settingsOwner: ObsidianSettingsOwner;
  private readonly stateOwner: ObsidianStateOwner;
  private readonly sourcePanelOwner: ObsidianSourcePanelOwner;
  private readonly sourceReferencesOwner: ObsidianSourceReferencesOwner;
  private stateStatusSubscription: DisposableSubscription | null = null;

  constructor(
    app: App,
    manifest: PluginManifest,
    loadWasmRevision: (raw: string) => Promise<CrivLoadedState> = loadWasmState,
  ) {
    super(app, manifest);
    this.settingsOwner = new ObsidianSettingsOwner(app, this, () => this.stateOwner.loadState());
    this.stateOwner = new ObsidianStateOwner(
      app,
      () => this.settings?.statePath ?? this.settingsOwner.settings.statePath,
      loadWasmRevision,
    );
    this.sourcePanelOwner = new ObsidianSourcePanelOwner(
      app,
      this.stateOwner,
      () => this.settings?.externalEditorUrl ?? this.settingsOwner.settings.externalEditorUrl,
    );
    this.sourceReferencesOwner = new ObsidianSourceReferencesOwner(app, this.stateOwner, (path) =>
      this.sourcePanelOwner.openExternal(path),
    );
  }

  async onload(): Promise<void> {
    await this.settingsOwner.load();
    this.settings = this.settingsOwner.settings;
    const c4Port: C4ViewPort = {
      currentStateStatus: () => this.stateOwner.currentStateStatus(),
      onStateStatusChange: (listener) => this.stateOwner.onStateStatusChange(listener),
      openValidatedSource: (target) => this.sourceReferencesOwner.openValidatedSource(target),
    };

    this.addRibbonIcon("network", "criv status", async () => this.stateOwner.showStatus());
    this.addCommand({
      id: "show-criv-status",
      name: "Show criv status",
      callback: async () => this.stateOwner.showStatus(),
    });
    this.addCommand({
      id: "open-criv-source-panel",
      name: "Open criv source panel",
      callback: async () => this.sourcePanelOwner.open(),
    });
    this.addCommand({
      id: "reload-criv-state",
      name: "Reload criv state",
      callback: async () => this.stateOwner.loadState(),
    });
    patchNativeSaveCommand(this.app, (cleanup) => this.register(cleanup));
    this.registerEditorExtension(this.sourceReferencesOwner.editorExtension());
    this.registerView(SOURCE_PANEL_VIEW_TYPE, (leaf) => this.sourcePanelOwner.createView(leaf));
    this.registerView(C4_VIEW_TYPE, (leaf) => new CrivC4View(leaf, c4Port));
    this.registerExtensions(["c4"], C4_VIEW_TYPE);
    this.registerMarkdownPostProcessor((el, ctx) =>
      this.sourceReferencesOwner.decorateLinks(el, ctx),
    );
    this.registerDomEvent(document, "mouseover", (event) =>
      this.sourceReferencesOwner.handleDocumentMouseOver(event),
    );
    this.registerDomEvent(document, "mouseout", (event) =>
      this.sourceReferencesOwner.handleDocumentMouseOut(event),
    );
    this.registerEditorSuggest(this.sourceReferencesOwner.createSuggest());
    this.registerEvent(
      this.app.workspace.on("active-leaf-change", () => this.sourcePanelOwner.refresh()),
    );
    this.registerEvent(this.app.metadataCache.on("changed", () => this.sourcePanelOwner.refresh()));
    this.stateStatusSubscription = this.stateOwner.onStateStatusChange((status) => {
      if (status.kind === "loading") {
        return;
      }
      this.app.workspace.updateOptions();
      void this.sourcePanelOwner.refresh();
    });
    this.registerInterval(
      window.setInterval(() => {
        void this.stateOwner.observeFile();
      }, STATE_POLL_INTERVAL_MS),
    );
    this.addSettingTab(this.settingsOwner.createTab());
    this.app.workspace.onLayoutReady(() => {
      void this.stateOwner.loadState();
      void this.sourcePanelOwner.ensure(false);
    });
  }

  onunload(): void {
    shutdownC4Views(this.app);
    this.stateStatusSubscription?.dispose();
    this.stateStatusSubscription = null;
    this.sourceReferencesOwner.dispose();
    this.stateOwner.dispose();
  }

  async loadState(): Promise<CrivState | null> {
    return this.stateOwner.loadState();
  }

  async getState(): Promise<CrivState | null> {
    return this.stateOwner.getState();
  }

  async readState(): Promise<CrivStateSummary | null> {
    return this.stateOwner.readState();
  }

  cachedState(): CrivState | null {
    return this.stateOwner.cachedState();
  }

  stateStatus(): string {
    return this.stateOwner.stateStatus();
  }

  currentStateStatus(): ObsidianStateStatus {
    return this.stateOwner.currentStateStatus();
  }

  onStateStatusChange(listener: (status: ObsidianStateStatus) => void): DisposableSubscription {
    return this.stateOwner.onStateStatusChange(listener);
  }

  pollState = async (): Promise<void> => this.stateOwner.observeFile();

  updateStatePath = async (value: string): Promise<void> => {
    this.settingsOwner.replace(this.settings);
    await this.settingsOwner.setStatePath(value);
  };

  openValidatedSource(target: string): void {
    this.sourceReferencesOwner.openValidatedSource(target);
  }
}

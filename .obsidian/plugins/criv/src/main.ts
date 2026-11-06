import { Plugin } from "obsidian";
import type { App, PluginManifest } from "obsidian";
import { ObsidianSettingsOwner } from "./settings";
import { ObsidianSourcePanelOwner, SOURCE_PANEL_VIEW_TYPE } from "./source/panel";
import { ObsidianSourceReferencesOwner } from "./source/references";
import { ObsidianStateOwner } from "./state/owner";

const STATE_POLL_INTERVAL_MS = 2_000;

export default class CrivPlugin extends Plugin {
  private readonly settingsOwner: ObsidianSettingsOwner;
  private readonly state: ObsidianStateOwner;
  private readonly sourcePanel: ObsidianSourcePanelOwner;
  private readonly sourceReferences: ObsidianSourceReferencesOwner;

  constructor(app: App, manifest: PluginManifest) {
    super(app, manifest);
    this.settingsOwner = new ObsidianSettingsOwner(app, this, () => this.state.refresh());
    this.state = new ObsidianStateOwner(app, () => this.settingsOwner.current.statePath);
    this.sourcePanel = new ObsidianSourcePanelOwner(app, this.state, (absolutePath) =>
      this.settingsOwner.externalEditorUrl(absolutePath),
    );
    this.sourceReferences = new ObsidianSourceReferencesOwner(app, this.state);
  }

  async onload(): Promise<void> {
    await this.settingsOwner.load();
    this.addRibbonIcon("network", "criv status", async () => this.state.showStatus());
    this.addCommand({
      id: "show-criv-status",
      name: "Show criv status",
      callback: async () => this.state.showStatus(),
    });
    this.addCommand({
      id: "open-criv-source-panel",
      name: "Open criv source panel",
      callback: async () => this.sourcePanel.open(),
    });
    this.addCommand({
      id: "reload-criv-state",
      name: "Reload criv state",
      callback: async () => this.state.refresh(),
    });
    this.registerEditorExtension(this.sourceReferences.editorExtension());
    this.registerView(SOURCE_PANEL_VIEW_TYPE, (leaf) => this.sourcePanel.createView(leaf));
    this.registerMarkdownPostProcessor((el) => this.sourceReferences.decorateLinks(el));
    this.registerDomEvent(document, "mouseover", (event) =>
      this.sourceReferences.handleDocumentMouseOver(event),
    );
    this.registerDomEvent(document, "mouseout", (event) =>
      this.sourceReferences.handleDocumentMouseOut(event),
    );
    this.registerEditorSuggest(this.sourceReferences.createSuggest());
    this.registerEvent(
      this.app.workspace.on("active-leaf-change", () => this.sourcePanel.refresh()),
    );
    this.registerEvent(this.app.metadataCache.on("changed", () => this.sourcePanel.refresh()));
    this.state.onStatusChange((status) => {
      if (status.kind !== "loading") {
        this.app.workspace.updateOptions();
        void this.sourcePanel.refresh();
      }
    });
    this.registerInterval(window.setInterval(() => void this.state.poll(), STATE_POLL_INTERVAL_MS));
    this.addSettingTab(this.settingsOwner.createTab());
    this.app.workspace.onLayoutReady(() => {
      void this.state.refresh();
      void this.sourcePanel.ensure(false);
    });
  }

  onunload(): void {
    this.sourceReferences.dispose();
    this.state.dispose();
  }
}

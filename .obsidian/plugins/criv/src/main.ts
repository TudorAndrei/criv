import { Notice, Plugin, PluginSettingTab, Setting } from "obsidian";
import { summarizeState } from "./wasm";

interface CrivSettings {
  statePath: string;
}

const DEFAULT_SETTINGS: CrivSettings = {
  statePath: ".criv/state.json",
};

export default class CrivPlugin extends Plugin {
  settings: CrivSettings;

  async onload() {
    this.settings = Object.assign({}, DEFAULT_SETTINGS, await this.loadData());
    this.addRibbonIcon("network", "criv status", async () => this.showStatus());
    this.addCommand({
      id: "show-criv-status",
      name: "Show criv status",
      callback: async () => this.showStatus(),
    });
    this.addSettingTab(new CrivSettingTab(this.app, this));
  }

  async showStatus() {
    const state = await this.readState();
    if (!state) {
      new Notice(`criv state is missing at ${this.settings.statePath}`);
      return;
    }

    new Notice(
      `criv ${state.schema}: ${state.node_count} nodes, ${state.edge_count} edges, ${state.source_count} source files`,
    );
  }

  async readState() {
    try {
      const raw = await this.app.vault.adapter.read(this.settings.statePath);
      return await summarizeState(raw);
    } catch (_err) {
      return null;
    }
  }

  async saveSettings() {
    await this.saveData(this.settings);
  }
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
  }
}

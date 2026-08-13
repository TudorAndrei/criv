import { PluginSettingTab, Setting } from "obsidian";
import type { App, Plugin } from "obsidian";
import { safeVaultPath } from "./source/model";

export interface CrivSettings {
  statePath: string;
  externalEditorUrl: string;
}

export const DEFAULT_SETTINGS: CrivSettings = {
  statePath: ".criv/state.json",
  externalEditorUrl: "vscode://file/{path}",
};

export class ObsidianSettingsOwner {
  settings: CrivSettings = { ...DEFAULT_SETTINGS };

  constructor(
    private readonly app: App,
    private readonly plugin: Plugin,
    private readonly reloadState: () => Promise<unknown>,
  ) {}

  async load(): Promise<void> {
    this.settings = Object.assign({}, DEFAULT_SETTINGS, await this.plugin.loadData());
  }

  replace(settings: CrivSettings): void {
    this.settings = settings;
  }

  async setStatePath(value: string): Promise<void> {
    this.settings.statePath = safeVaultPath(value) ?? DEFAULT_SETTINGS.statePath;
    await this.save();
    await this.reloadState();
  }

  async updateExternalEditorUrl(value: string): Promise<void> {
    this.settings.externalEditorUrl = value.trim() || DEFAULT_SETTINGS.externalEditorUrl;
    await this.save();
  }

  async save(): Promise<void> {
    await this.plugin.saveData(this.settings);
  }

  createTab(): PluginSettingTab {
    return new CrivSettingTab(this.app, this.plugin, this);
  }
}

class CrivSettingTab extends PluginSettingTab {
  constructor(
    app: App,
    plugin: Plugin,
    private readonly owner: ObsidianSettingsOwner,
  ) {
    super(app, plugin);
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
          .setValue(this.owner.settings.statePath)
          .onChange(async (value) => this.owner.setStatePath(value)),
      );

    new Setting(containerEl)
      .setName("External editor URL")
      .setDesc(
        "URL template for opening source files. Use {path} for the repo-relative source path.",
      )
      .addText((text) =>
        text
          .setPlaceholder("vscode://file/{path}")
          .setValue(this.owner.settings.externalEditorUrl)
          .onChange(async (value) => this.owner.updateExternalEditorUrl(value)),
      );
  }
}

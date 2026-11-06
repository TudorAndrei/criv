import { PluginSettingTab, Setting } from "obsidian";
import type { App, Plugin } from "obsidian";
import { CRIV_STATE_PATH, safeVaultPath } from "@criv/editor-state";

export interface CrivSettings {
  statePath: string;
  externalEditorUrl: string;
}

const DEFAULT_SETTINGS: CrivSettings = {
  statePath: CRIV_STATE_PATH,
  externalEditorUrl: "vscode://file/{path}",
};

/** The one owner of persisted plugin settings and the settings tab. */
export class ObsidianSettingsOwner {
  private settings: CrivSettings = { ...DEFAULT_SETTINGS };

  constructor(
    private readonly app: App,
    private readonly plugin: Plugin,
    private readonly reloadState: () => Promise<unknown>,
  ) {}

  get current(): Readonly<CrivSettings> {
    return this.settings;
  }

  async load(): Promise<void> {
    this.settings = { ...DEFAULT_SETTINGS, ...(await this.plugin.loadData()) };
  }

  async setStatePath(value: string): Promise<void> {
    this.settings.statePath = safeVaultPath(value) ?? DEFAULT_SETTINGS.statePath;
    await this.plugin.saveData(this.settings);
    await this.reloadState();
  }

  async setExternalEditorUrl(value: string): Promise<void> {
    this.settings.externalEditorUrl = value.trim() || DEFAULT_SETTINGS.externalEditorUrl;
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
          .setPlaceholder(CRIV_STATE_PATH)
          .setValue(this.owner.current.statePath)
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
          .setValue(this.owner.current.externalEditorUrl)
          .onChange(async (value) => this.owner.setExternalEditorUrl(value)),
      );
  }
}

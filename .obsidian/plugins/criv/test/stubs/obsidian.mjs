export class Plugin {
  constructor(app, manifest) {
    this.app = app;
    this.manifest = manifest;
  }

  async loadData() {
    return {};
  }

  async saveData() {}

  addRibbonIcon() {}
  addCommand() {}
  addSettingTab() {}
  register() {}
  registerDomEvent() {}
  registerEditorExtension() {}
  registerEditorSuggest() {}
  registerEvent() {}
  registerExtensions() {}
  registerMarkdownPostProcessor() {}
  registerView() {}
}

export class ItemView {
  constructor(leaf) {
    this.leaf = leaf;
    this.app = leaf?.app;
    this.containerEl = { children: [] };
  }
}

export class FileView extends ItemView {}

export class EditorSuggest {
  constructor(app) {
    this.app = app;
  }
}

export class PluginSettingTab {
  constructor(app, plugin) {
    this.app = app;
    this.plugin = plugin;
    this.containerEl = { empty() {} };
  }
}

export class Setting {
  constructor(containerEl) {
    this.containerEl = containerEl;
  }

  setName() {
    return this;
  }

  setDesc() {
    return this;
  }

  addText(callback) {
    callback?.({
      setPlaceholder() {
        return this;
      },
      setValue() {
        return this;
      },
      onChange() {
        return this;
      },
    });
    return this;
  }
}

export class Notice {
  constructor(message) {
    Notice.messages.push(message);
  }
}

Notice.messages = [];

export class TFile {}
export class WorkspaceLeaf {}

/** The Obsidian API surface the plugin uses, recording what the plugin registers. */
interface Command {
  id: string;
  callback: () => unknown;
}

export class Plugin {
  readonly commands = new Map<string, () => unknown>();
  readonly intervals: number[] = [];
  data: unknown = {};
  settingTab: PluginSettingTab | undefined;
  readonly app: unknown;

  constructor(app: unknown, _manifest: unknown) {
    this.app = app;
  }

  async loadData(): Promise<unknown> {
    return this.data;
  }

  async saveData(data: unknown): Promise<void> {
    this.data = structuredClone(data);
  }

  addCommand(command: Command): void {
    this.commands.set(command.id, command.callback);
  }

  addSettingTab(tab: PluginSettingTab): void {
    this.settingTab = tab;
  }

  registerInterval(id: number): number {
    this.intervals.push(id);
    return id;
  }

  addRibbonIcon(): void {}
  register(): void {}
  registerDomEvent(): void {}
  registerEditorExtension(): void {}
  registerEditorSuggest(): void {}
  registerEvent(): void {}
  registerMarkdownPostProcessor(): void {}
  registerView(): void {}
}

export class ItemView {
  readonly containerEl = { children: [] as unknown[] };

  constructor(readonly leaf: unknown) {}
}

export class EditorSuggest {
  constructor(readonly app: unknown) {}
}

export class PluginSettingTab {
  readonly containerEl = { empty() {} };

  constructor(
    readonly app: unknown,
    readonly plugin: unknown,
  ) {}

  display(): void {}
}

/** Each text field's change handler, by setting name. */
export const settingHandlers = new Map<string, (value: string) => Promise<unknown>>();

export class Setting {
  private name = "";

  constructor(_containerEl: unknown) {}

  setName(name: string): this {
    this.name = name;
    return this;
  }

  setDesc(): this {
    return this;
  }

  addText(build: (text: TextField) => unknown): this {
    build(new TextField((handler) => settingHandlers.set(this.name, handler)));
    return this;
  }
}

class TextField {
  constructor(private readonly register: (handler: (value: string) => Promise<unknown>) => void) {}

  setPlaceholder(): this {
    return this;
  }

  setValue(): this {
    return this;
  }

  onChange(handler: (value: string) => Promise<unknown>): this {
    this.register(handler);
    return this;
  }
}

export class Notice {
  static readonly messages: string[] = [];

  constructor(message: string) {
    Notice.messages.push(message);
  }
}

export class TFile {
  path = "";
}

/** The desktop vault adapter, rooted at `basePath`. */
export class FileSystemAdapter {
  constructor(private readonly basePath: string) {}

  getFullPath(normalizedPath: string): string {
    return `${this.basePath}/${normalizedPath}`;
  }
}

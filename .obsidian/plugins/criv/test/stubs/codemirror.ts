/** The CodeMirror API surface the plugin uses; Obsidian provides the real modules. */
export class RangeSetBuilder {
  add(): void {}

  finish(): unknown[] {
    return [];
  }
}

export const Decoration = {
  none: [],
  mark(value: unknown): unknown {
    return value;
  },
};

export const ViewPlugin = {
  fromClass() {
    return { of: (value: unknown) => value };
  },
};

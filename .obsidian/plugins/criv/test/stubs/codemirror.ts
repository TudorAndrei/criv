/** The CodeMirror API surface the plugin uses; Obsidian provides the real modules. */
export class RangeSetBuilder {
  private readonly ranges: { from: number; to: number; value: unknown }[] = [];

  add(from: number, to: number, value: unknown): void {
    this.ranges.push({ from, to, value });
  }

  finish(): unknown[] {
    return this.ranges;
  }
}

export const Decoration = {
  none: [],
  mark(value: unknown): unknown {
    return value;
  },
};

/** `fromClass(...).of(arg)` returns the plugin class and its argument for a test to construct. */
export const ViewPlugin = {
  fromClass(plugin: unknown) {
    return { of: (arg: unknown) => ({ plugin, arg }) };
  },
};

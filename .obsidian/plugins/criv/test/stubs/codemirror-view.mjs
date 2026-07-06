export const Decoration = {
  mark(value) {
    return value;
  },
  none: [],
};

export const ViewPlugin = {
  fromClass() {
    return {
      of(value) {
        return value;
      },
    };
  },
};

export class EditorView {}

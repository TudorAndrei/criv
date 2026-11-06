import { resolve } from "node:path";

import { bundleTests } from "@criv/editor-state/esbuild";

const root = resolve(import.meta.dirname, "..");
const testRoot = resolve(root, "test");
const stubs = {
  obsidian: resolve(testRoot, "stubs/obsidian.ts"),
  "@codemirror/state": resolve(testRoot, "stubs/codemirror.ts"),
  "@codemirror/view": resolve(testRoot, "stubs/codemirror.ts"),
};

const obsidianStubs = {
  name: "obsidian-stubs",
  setup(build) {
    build.onResolve({ filter: /^(obsidian|@codemirror\/(state|view))$/ }, (args) => ({
      path: stubs[args.path],
    }));
  },
};

await bundleTests({
  testRoot,
  outdir: resolve(root, "dist-test"),
  entryName: /\.test\.ts$/,
  outExtension: { ".js": ".cjs" },
  target: "node22",
  plugins: [obsidianStubs],
});

import { resolve } from "node:path";
import process from "node:process";

import { bundleTests } from "@criv/editor-state/esbuild";

const root = resolve(import.meta.dirname, "..");
const suite = process.argv.includes("--integration") ? "integration" : "unit";

await bundleTests({
  testRoot: resolve(root, "test", suite),
  outdir: resolve(root, "dist-test", suite),
  entryName: suite === "unit" ? /\.test\.ts$/ : /^runner\.ts$/,
  target: "node18",
  external: ["vscode"],
});

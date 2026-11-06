import { builtinModules } from "node:module";
import process from "node:process";

import { crivWasmBytesPlugin } from "@criv/editor-state/esbuild";
import esbuild from "esbuild";

const args = new Set(process.argv.slice(2));
const production = args.has("--production");

const context = await esbuild.context({
  entryPoints: ["src/extension.ts"],
  bundle: true,
  format: "cjs",
  platform: "node",
  target: "node18",
  external: ["vscode", ...builtinModules, ...builtinModules.map((name) => `node:${name}`)],
  plugins: [crivWasmBytesPlugin],
  outfile: "dist/extension.js",
  sourcemap: production ? false : "inline",
  sourcesContent: !production,
  minify: production,
  logLevel: "info",
});

if (args.has("--watch")) {
  await context.watch();
} else {
  await context.rebuild();
  await context.dispose();
}

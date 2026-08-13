import esbuild from "esbuild";
import process from "node:process";
import { builtinModules } from "node:module";

const args = new Set(process.argv.slice(2));
const production = args.has("--production");
const watch = args.has("--watch");

const context = await esbuild.context({
  entryPoints: ["src/extension.ts"],
  bundle: true,
  format: "cjs",
  platform: "node",
  target: "node18",
  external: [
    "vscode",
    "../../pkg/criv_wasm.js",
    ...builtinModules,
    ...builtinModules.map((name) => `node:${name}`),
  ],
  outfile: "dist/extension.js",
  sourcemap: production ? false : "inline",
  sourcesContent: !production,
  minify: production,
  logLevel: "info",
});

const webviewContext = await esbuild.context({
  entryPoints: ["src/c4/webview.ts"],
  bundle: true,
  format: "iife",
  platform: "browser",
  target: "es2022",
  alias: {
    "node:module": "./src/nodeModuleShim.ts",
  },
  outfile: "media/likec4-preview.js",
  sourcemap: false,
  minify: production,
  logLevel: "info",
});

if (watch) {
  await Promise.all([context.watch(), webviewContext.watch()]);
} else {
  await Promise.all([context.rebuild(), webviewContext.rebuild()]);
  await Promise.all([context.dispose(), webviewContext.dispose()]);
}

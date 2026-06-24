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
  external: ["vscode", ...builtinModules, ...builtinModules.map((name) => `node:${name}`)],
  outfile: "dist/extension.js",
  sourcemap: production ? false : "inline",
  sourcesContent: !production,
  minify: production,
  logLevel: "info",
});

if (watch) {
  await context.watch();
} else {
  await context.rebuild();
  await context.dispose();
}

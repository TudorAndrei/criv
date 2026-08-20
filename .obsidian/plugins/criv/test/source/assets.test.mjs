import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "../..");
const outFile = resolve(tmpdir(), `criv-assets-test-${process.pid}.mjs`);
const panelOutFile = resolve(tmpdir(), `criv-asset-panel-test-${process.pid}.mjs`);

mkdirSync(dirname(outFile), { recursive: true });
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/source/assets.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
});

const assets = await import(pathToFileURL(outFile).href);
const image = {
  path: "docs/diagram.png",
  mime: "image/png",
  bytes: 128,
  hash: "a".repeat(64),
};
const pdf = {
  path: "docs/report.pdf",
  mime: "application/pdf",
  bytes: 256,
  hash: "b".repeat(64),
};

assert.equal(assets.isPassiveImage(image), true);
assert.equal(assets.isPassiveImage(pdf), false);
assert.equal(
  assets.assetResourceUrl("app://vault/docs/diagram.png", image.hash),
  `app://vault/docs/diagram.png?criv-asset=${image.hash}`,
);
assert.equal(
  assets.assetResourceUrl("app://vault/docs/diagram.png?existing=1", "a b"),
  "app://vault/docs/diagram.png?existing=1&criv-asset=a%20b",
);

assert.equal(assets.resolveActiveAsset([image], "docs/diagram.png"), image);
for (const path of ["docs/missing.png", "../diagram.png", "/tmp/diagram.png", ""]) {
  assert.equal(assets.resolveActiveAsset([image], path), null);
}

let removed = false;
let showedError = false;
assets.replaceFailedAssetPreview(
  {
    querySelector: () => ({ remove: () => (removed = true) }),
  },
  () => (showedError = true),
);
assert.equal(removed, true);
assert.equal(showedError, true);

await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/source/panel.ts")],
  outfile: panelOutFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
  plugins: [
    {
      name: "obsidian-test-stub",
      setup(build) {
        build.onResolve({ filter: /^obsidian$/ }, () => ({
          path: pathToFileURL(resolve(pluginRoot, "test/stubs/obsidian.mjs")).href,
          external: true,
        }));
      },
    },
  ],
});

const { ObsidianSourcePanelOwner } = await import(pathToFileURL(panelOutFile).href);
const { TFile } = await import(pathToFileURL(resolve(pluginRoot, "test/stubs/obsidian.mjs")).href);
const file = new TFile();
file.path = image.path;
const opened = [];
const owner = new ObsidianSourcePanelOwner(
  {
    vault: {
      getAbstractFileByPath: (path) => (path === file.path ? file : null),
      getResourcePath: () => "app://vault/docs/diagram.png",
    },
    workspace: {
      getLeaf: () => ({ openFile: async (selected) => opened.push(selected) }),
    },
  },
  { assetEntries: async () => [image] },
  () => "",
);

await owner.openAsset(image.path);
assert.deepEqual(opened, [file]);
assert.equal(
  await owner.assetResourcePath(image.path),
  `app://vault/docs/diagram.png?criv-asset=${image.hash}`,
);
await assert.rejects(owner.openAsset("docs/missing.png"), /Could not open/);
assert.equal(await owner.assetResourcePath("docs/missing.png"), null);
assert.deepEqual(opened, [file]);

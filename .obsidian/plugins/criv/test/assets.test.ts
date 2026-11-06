import assert from "node:assert/strict";
import test from "node:test";
import type { CrivAssetEntry, CrivInitialProjections } from "@criv/editor-state";
import type { App } from "obsidian";

import type { StatePort } from "../src/ports";
import { assetResourceUrl, resolveActiveAsset } from "../src/source/assets";
import { ObsidianSourcePanelOwner } from "../src/source/panel";
import { TFile } from "./stubs/obsidian";

const image: CrivAssetEntry = {
  path: "docs/diagram.png",
  mime: "image/png",
  bytes: 128,
  hash: "a".repeat(64),
};

test("resolves only a safe path from the active asset inventory", () => {
  assert.equal(resolveActiveAsset([image], "./docs\\diagram.png"), image);
  for (const path of ["docs/missing.png", "../diagram.png", "/tmp/diagram.png", "", 7]) {
    assert.equal(resolveActiveAsset([image], path), null, String(path));
  }
});

test("adds the asset hash to its resource URL", () => {
  assert.equal(
    assetResourceUrl("app://vault/docs/diagram.png", image.hash),
    `app://vault/docs/diagram.png?criv-asset=${image.hash}`,
  );
  assert.equal(
    assetResourceUrl("app://vault/docs/diagram.png?existing=1", "a b"),
    "app://vault/docs/diagram.png?existing=1&criv-asset=a%20b",
  );
});

test("the source panel opens and previews only assets in the loaded State", async () => {
  const file = Object.assign(new TFile(), { path: image.path });
  const opened: unknown[] = [];
  const app = {
    vault: {
      getAbstractFileByPath: (path: string) => (path === file.path ? file : null),
      getResourcePath: () => "app://vault/docs/diagram.png",
    },
    workspace: {
      getLeaf: () => ({ openFile: async (selected: unknown) => opened.push(selected) }),
    },
  } as unknown as App;
  const state = {
    load: async () => ({ assets: [image] }) as unknown as CrivInitialProjections,
  } as StatePort;
  const owner = new ObsidianSourcePanelOwner(app, state, () => "");

  await owner.openAsset(image.path);
  await assert.rejects(owner.openAsset("docs/missing.png"), /Could not open docs\/missing.png/);

  assert.deepEqual(opened, [file]);
  assert.equal(
    await owner.assetResourcePath(image.path),
    `app://vault/docs/diagram.png?criv-asset=${image.hash}`,
  );
  assert.equal(await owner.assetResourcePath("docs/missing.png"), null);
});

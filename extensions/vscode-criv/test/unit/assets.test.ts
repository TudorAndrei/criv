import assert from "node:assert/strict";
import test from "node:test";

import { assetTreePresentation, openActiveAsset, planAssetOpen } from "../../src/assets";

const assets = [
  {
    path: "docs/diagram.png",
    mime: "image/png",
    bytes: 128,
    hash: "a".repeat(64),
  },
];

test("authorizes only a safe path from the active asset inventory", () => {
  assert.deepEqual(planAssetOpen(assets, "docs/diagram.png"), {
    kind: "resolved",
    asset: assets[0],
  });
  assert.deepEqual(planAssetOpen(assets, "docs/missing.png"), { kind: "unauthorized" });
});

test("rejects asset paths that can escape the workspace", () => {
  for (const path of ["../diagram.png", "/tmp/diagram.png", "C:\\tmp\\diagram.png", ""]) {
    assert.deepEqual(planAssetOpen(assets, path), { kind: "invalid" });
  }
});

test("opens only an authorized asset through the native adapter", async () => {
  const opened: string[] = [];
  assert.deepEqual(
    await openActiveAsset(assets, "docs/diagram.png", async (asset) => {
      opened.push(asset.path);
    }),
    { kind: "opened", asset: assets[0] },
  );
  assert.deepEqual(await openActiveAsset(assets, "docs/missing.png", async () => {}), {
    kind: "unauthorized",
  });
  assert.deepEqual(opened, ["docs/diagram.png"]);
});

test("builds the documentation asset tree row", () => {
  assert.deepEqual(assetTreePresentation(assets[0]!), {
    label: "docs/diagram.png",
    description: "image/png · 128 B",
    command: "criv.openAsset",
    arguments: ["docs/diagram.png"],
  });
});

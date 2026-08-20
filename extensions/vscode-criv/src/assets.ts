import { safeVaultPath } from "./navigation/target";
import type { CrivAssetEntry } from "./state/wasm";

export type AssetOpenPlan =
  | { kind: "resolved"; asset: CrivAssetEntry }
  | { kind: "invalid" }
  | { kind: "unauthorized" };

export type AssetOpenResult =
  | Exclude<AssetOpenPlan, { kind: "resolved" }>
  | { kind: "opened"; asset: CrivAssetEntry }
  | { kind: "failed"; asset: CrivAssetEntry; error: unknown };

export function planAssetOpen(
  assets: readonly CrivAssetEntry[],
  requestedPath: unknown,
): AssetOpenPlan {
  const path = safeVaultPath(requestedPath);
  if (!path) {
    return { kind: "invalid" };
  }
  const asset = assets.find((entry) => entry.path === path);
  return asset ? { kind: "resolved", asset } : { kind: "unauthorized" };
}

export async function openActiveAsset(
  assets: readonly CrivAssetEntry[],
  requestedPath: unknown,
  openNative: (asset: CrivAssetEntry) => Promise<void>,
): Promise<AssetOpenResult> {
  const plan = planAssetOpen(assets, requestedPath);
  if (plan.kind !== "resolved") {
    return plan;
  }
  try {
    await openNative(plan.asset);
    return { kind: "opened", asset: plan.asset };
  } catch (error) {
    return { kind: "failed", asset: plan.asset, error };
  }
}

export function assetTreePresentation(asset: CrivAssetEntry): {
  label: string;
  description: string;
  command: string;
  arguments: [string];
} {
  return {
    label: asset.path,
    description: `${asset.mime} · ${formatBytes(asset.bytes)}`,
    command: "criv.openAsset",
    arguments: [asset.path],
  };
}

function formatBytes(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`;
}

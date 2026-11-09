import type { CrivAssetEntry } from "./contract.ts";
import { safeVaultPath } from "./sourceTarget.ts";

export type AssetLookup =
  | { kind: "resolved"; asset: CrivAssetEntry }
  | { kind: "invalid" }
  | { kind: "unauthorized" };

/** The asset in the loaded inventory at a requested path; only listed assets may open. */
export function lookupAsset(
  assets: readonly CrivAssetEntry[],
  requestedPath: unknown,
): AssetLookup {
  const path = safeVaultPath(requestedPath);
  if (!path) {
    return { kind: "invalid" };
  }
  const asset = assets.find((entry) => entry.path === path);
  return asset ? { kind: "resolved", asset } : { kind: "unauthorized" };
}

import { readFileSync, writeFileSync } from "fs";
import { pathToFileURL } from "node:url";

export function bumpedVersions(versions, targetVersion, minAppVersion) {
  if (versions[targetVersion] === minAppVersion) {
    return null;
  }
  return { ...versions, [targetVersion]: minAppVersion };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const targetVersion = process.env.npm_package_version;
  const manifest = JSON.parse(readFileSync("manifest.json", "utf8"));
  const { minAppVersion } = manifest;
  manifest.version = targetVersion;
  writeFileSync("manifest.json", JSON.stringify(manifest, null, "\t"));

  const versions = JSON.parse(readFileSync("versions.json", "utf8"));
  const updated = bumpedVersions(versions, targetVersion, minAppVersion);
  if (updated) {
    writeFileSync("versions.json", JSON.stringify(updated, null, "\t"));
  }
}

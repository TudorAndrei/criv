# Plan 003: Make the Obsidian plugin version bump always record the new version in versions.json

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- .obsidian/plugins/criv/version-bump.mjs .obsidian/plugins/criv/package.json .obsidian/plugins/criv/test`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

Obsidian uses a plugin's `versions.json` (a map of plugin version →
minimum app version) to decide which plugin release to offer users. The
version-bump script that maintains this file has an inverted check: it asks
whether the `minAppVersion` **value** already appears anywhere in the map,
instead of whether the new plugin version **key** is present. Since most
releases keep the same `minAppVersion`, the common case writes `manifest.json`
but silently skips `versions.json` — so every release after the first with the
same floor is missing from the compatibility map. This was flagged in a prior
audit (2026-06-21, ISSUES.md issue 3) and is still unfixed.

## Current state

Relevant files:

- `.obsidian/plugins/criv/version-bump.mjs` — the whole script (13 lines),
  run by the npm `version` lifecycle:
  `package.json` has `"version": "node version-bump.mjs && git add manifest.json versions.json"`.
- `.obsidian/plugins/criv/manifest.json` — currently `"version": "0.1.0"`,
  `"minAppVersion": "1.5.0"`.
- `.obsidian/plugins/criv/versions.json` — currently `{"0.1.0": "1.5.0"}`.
- `.obsidian/plugins/criv/test/core.test.mjs` — the only plugin test file;
  `package.json` `"test": "node test/core.test.mjs"`. It is a plain
  `node:assert/strict` script (no test framework).

The full current script (`version-bump.mjs`):

```js
import { readFileSync, writeFileSync } from "fs";

const targetVersion = process.env.npm_package_version;
const manifest = JSON.parse(readFileSync("manifest.json", "utf8"));
const { minAppVersion } = manifest;
manifest.version = targetVersion;
writeFileSync("manifest.json", JSON.stringify(manifest, null, "\t"));

const versions = JSON.parse(readFileSync("versions.json", "utf8"));
if (!Object.values(versions).includes(minAppVersion)) {
  versions[targetVersion] = minAppVersion;
  writeFileSync("versions.json", JSON.stringify(versions, null, "\t"));
}
```

The bug is the `if` condition. Demonstration with current data: a bump to
`0.2.0` keeps `minAppVersion` `1.5.0`; `Object.values({"0.1.0":"1.5.0"})`
already includes `"1.5.0"`, so `0.2.0` is never written.

Repo conventions: plugin JS is linted by oxlint and formatted by oxfmt
(`npm run lint`, `npm run format:check` in the plugin directory); conventional
commit messages.

## Commands you will need

Run all npm commands with `--prefix .obsidian/plugins/criv` from the repo
root (or cd into the plugin directory).

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `npm --prefix .obsidian/plugins/criv test` | exit 0 |
| Lint | `npm --prefix .obsidian/plugins/criv run lint` | exit 0 |
| Format | `npm --prefix .obsidian/plugins/criv run format:check` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `.obsidian/plugins/criv/version-bump.mjs`
- `.obsidian/plugins/criv/test/version-bump.test.mjs` (create)
- `.obsidian/plugins/criv/package.json` (only the `test` script line)

**Out of scope** (do NOT touch):
- `.obsidian/plugins/criv/manifest.json` and `versions.json` — the real data
  files; tests must not mutate them.
- `extensions/vscode-criv/**` — the VS Code extension has no equivalent
  script.
- Any release automation in `scripts/` or `cog.toml`.

## Git workflow

- Conventional commits; single commit, e.g.
  `fix(obsidian): record every plugin version in versions.json`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Extract a pure function and fix the predicate

Rewrite `version-bump.mjs` so the decision logic is an exported pure function
and file I/O only happens when the script is executed directly:

```js
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
```

Semantics: the new version key is always written unless it already maps to the
same `minAppVersion`; an existing key with a *different* floor is updated.

**Verify**: `npm --prefix .obsidian/plugins/criv run lint` → exit 0.

### Step 2: Add the test file and wire it into `npm test`

Create `.obsidian/plugins/criv/test/version-bump.test.mjs` as a plain assert
script (same style as `test/core.test.mjs` — `node:assert/strict`, top-level
asserts, no framework):

```js
import assert from "node:assert/strict";

import { bumpedVersions } from "../version-bump.mjs";

// New release keeping the same minAppVersion gets an entry (the bug this guards).
assert.deepEqual(bumpedVersions({ "0.1.0": "1.5.0" }, "0.2.0", "1.5.0"), {
  "0.1.0": "1.5.0",
  "0.2.0": "1.5.0",
});

// Re-running for an already-recorded version is a no-op.
assert.equal(bumpedVersions({ "0.1.0": "1.5.0" }, "0.1.0", "1.5.0"), null);

// A changed floor for an existing version is updated.
assert.deepEqual(bumpedVersions({ "0.1.0": "1.5.0" }, "0.1.0", "1.6.0"), {
  "0.1.0": "1.6.0",
});

console.log("version-bump tests passed");
```

Note: importing `../version-bump.mjs` must not trigger the file writes — that
is what the `import.meta.url` guard in Step 1 guarantees. If the import writes
or throws, Step 1 is wrong; fix it there.

Update the plugin `package.json` test script to run both files:

```json
"test": "node test/core.test.mjs && node test/version-bump.test.mjs",
```

**Verify**: `npm --prefix .obsidian/plugins/criv test` → exit 0, output
includes `version-bump tests passed`.

### Step 3: Verify the real data files are untouched and gate passes

**Verify**:
- `git status --short .obsidian/plugins/criv` lists only
  `version-bump.mjs`, `test/version-bump.test.mjs`, `package.json`
- `npm --prefix .obsidian/plugins/criv run lint` → exit 0
- `npm --prefix .obsidian/plugins/criv run format:check` → exit 0 (run
  `npm --prefix .obsidian/plugins/criv run format` first if it complains)

**Commit**: `fix(obsidian): record every plugin version in versions.json`

## Test plan

Covered in Step 2: the regression case (same floor, new version), the no-op
case, and the changed-floor case. Model: `test/core.test.mjs`.
Verification: `npm --prefix .obsidian/plugins/criv test` → exit 0.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `npm --prefix .obsidian/plugins/criv test` exits 0 and runs both test files
- [ ] `grep -n 'Object.values' .obsidian/plugins/criv/version-bump.mjs` returns
      no matches
- [ ] `git diff --stat` for `.obsidian/plugins/criv/manifest.json` and
      `versions.json` is empty
- [ ] lint and format:check exit 0
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `version-bump.mjs` no longer matches the excerpt above (drift — someone may
  have fixed it already; check `git log -- .obsidian/plugins/criv/version-bump.mjs`).
- oxlint rejects the `import.meta.url` main-module guard pattern and the fix
  isn't a trivial lint-config-compatible rephrasing.
- You find the npm `version` lifecycle is invoked anywhere in CI/release
  automation with assumptions about the old behavior (search `scripts/` and
  `.github/workflows/` for `npm version`) — report before changing semantics.

## Maintenance notes

- When the plugin is actually released to the Obsidian community catalog, this
  script is what keeps `versions.json` correct — reviewer should sanity-check
  the three semantic cases in the test.
- Deferred: no changes to how `manifest.json` is written; no validation that
  `npm_package_version` is set (the npm lifecycle guarantees it).

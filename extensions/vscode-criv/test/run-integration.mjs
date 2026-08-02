import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runTests } from "@vscode/test-electron";

const __dirname = dirname(fileURLToPath(import.meta.url));
const extensionRoot = resolve(__dirname, "..");
// macOS limits Unix-domain socket paths to 103 bytes. A worktree-local profile
// can exceed that limit before VS Code starts, so give each test run a short,
// isolated profile outside the checkout.
const userDataDir = await mkdtemp(join(tmpdir(), "criv-vscode-"));

await runTests({
  extensionDevelopmentPath: extensionRoot,
  extensionTestsPath: resolve(extensionRoot, "dist-test/integration/runner.js"),
  launchArgs: [`--user-data-dir=${userDataDir}`, resolve(extensionRoot, "../..")],
});

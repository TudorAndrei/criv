import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runTests } from "@vscode/test-electron";

const __dirname = dirname(fileURLToPath(import.meta.url));
const extensionRoot = resolve(__dirname, "..");

await runTests({
  extensionDevelopmentPath: extensionRoot,
  extensionTestsPath: resolve(extensionRoot, "dist-test/integration/runner.js"),
  launchArgs: [resolve(extensionRoot, "../..")],
});

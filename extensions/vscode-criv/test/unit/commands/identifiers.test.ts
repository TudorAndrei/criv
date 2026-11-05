import assert from "node:assert/strict";
import test from "node:test";

import extensionPackage from "../../../package.json";
import { CRIV_COMMANDS } from "../../../src/commands/identifiers";

test("the manifest contributes exactly the registered commands", () => {
  assert.deepEqual(
    extensionPackage.contributes.commands.map((command) => command.command).sort(),
    Object.values(CRIV_COMMANDS).sort(),
  );
});

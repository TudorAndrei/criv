import assert from "node:assert/strict";
import test from "node:test";

import {
  executablePathError,
  hasWorkspaceConfigurationValue,
  machineScopedValue,
} from "../../src/configModel";

test("machine scoped values ignore workspace and folder overrides", () => {
  assert.equal(
    machineScopedValue(
      {
        defaultValue: "criv",
        globalValue: "/usr/local/bin/criv",
        workspaceValue: "./tools/payload",
        workspaceFolderValue: "./folder/payload",
      },
      "criv",
    ),
    "/usr/local/bin/criv",
  );
  assert.equal(machineScopedValue({ defaultValue: false, workspaceValue: true }, false), false);
});

test("detects workspace configuration values", () => {
  assert.equal(hasWorkspaceConfigurationValue({ workspaceValue: "./payload" }), true);
  assert.equal(hasWorkspaceConfigurationValue({ workspaceFolderValue: "./payload" }), true);
  assert.equal(hasWorkspaceConfigurationValue({ globalValue: "criv" }), false);
});

test("rejects relative executable paths", () => {
  assert.equal(executablePathError("criv"), undefined);
  assert.equal(executablePathError("criv.exe"), undefined);
  assert.match(executablePathError("./tools/payload") ?? "", /command name on PATH/);
  assert.match(executablePathError("tools/payload") ?? "", /command name on PATH/);
  assert.match(executablePathError(" tools/payload ") ?? "", /whitespace/);
});

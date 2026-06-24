import assert from "node:assert/strict";

import * as vscode from "vscode";

const EXPECTED_COMMANDS = ["criv.refreshStateView", "criv.openStateJson", "criv.openSourceTarget"];

export async function run(): Promise<void> {
  const extension = vscode.extensions.getExtension("criv.vscode-criv");
  assert.ok(extension, "Expected the criv extension to be installed in the test host.");
  await extension.activate();

  const commands = await vscode.commands.getCommands(true);
  for (const command of EXPECTED_COMMANDS) {
    assert.ok(commands.includes(command), `Expected command ${command} to be registered.`);
  }
}

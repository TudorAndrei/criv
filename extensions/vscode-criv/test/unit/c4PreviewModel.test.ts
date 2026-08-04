import assert from "node:assert/strict";
import test from "node:test";

import { c4NavigationTarget, preferredC4ViewId } from "../../src/c4PreviewModel";

const views = [
  {
    id: "codeVscodePreview",
    title: "Code / VS Code / Preview",
    sourcePath: "code/vscode.c4",
  },
  {
    id: "codeVscodeState",
    title: "Code / VS Code / Workspace state",
    sourcePath: "code/vscode.c4",
  },
  {
    id: "cliComponents",
    title: "Components / CLI",
    sourcePath: "cli.c4",
  },
  {
    id: "obsidianComponents",
    title: "Components / Obsidian",
    sourcePath: "obsidian.c4",
  },
  {
    id: "index",
    title: "Overview / System context",
    sourcePath: "systems.c4",
  },
  {
    id: "unowned",
    title: "Overview / Unowned",
  },
];

test("selects the state view owned by the opened C4 file", () => {
  assert.equal(preferredC4ViewId("docs/architecture/systems.c4", views), "index");
});

test("selects a component view from its domain file", () => {
  assert.equal(preferredC4ViewId("docs/architecture/cli.c4", views), "cliComponents");
  assert.equal(preferredC4ViewId("docs/architecture/obsidian.c4", views), "obsidianComponents");
});

test("selects a Code view from its module model file", () => {
  assert.equal(preferredC4ViewId("docs/architecture/code/vscode.c4", views), "codeVscodePreview");
});

test("keeps navigation in a file that owns more than one focused view", () => {
  assert.equal(
    c4NavigationTarget(
      "docs/architecture/code/vscode.c4",
      "docs/architecture",
      "codeVscodeState",
      views,
    ),
    undefined,
  );
});

test("returns no preference when no state view belongs to the file", () => {
  assert.equal(preferredC4ViewId("docs/architecture/model.c4", views), undefined);
});

test("offers no view for a file that owns none", () => {
  assert.equal(preferredC4ViewId("docs/architecture/specification.c4", views), undefined);
});

test("navigation targets the file that owns the selected view", () => {
  assert.equal(
    c4NavigationTarget("docs/architecture/cli.c4", "docs/architecture", "index", views),
    "docs/architecture/systems.c4",
  );
  assert.equal(
    c4NavigationTarget(
      "docs/architecture/specification.c4",
      "docs/architecture",
      "cliComponents",
      views,
    ),
    "docs/architecture/cli.c4",
  );
});

test("navigation stays put when the open document already owns the view", () => {
  assert.equal(
    c4NavigationTarget("docs/architecture/cli.c4", "docs/architecture", "cliComponents", views),
    undefined,
  );
  assert.equal(
    c4NavigationTarget("./docs/architecture/cli.c4", "docs/architecture", "cliComponents", views),
    undefined,
  );
});

test("navigation stops when the view or the workspace has no path", () => {
  assert.equal(
    c4NavigationTarget("docs/architecture/cli.c4", "docs/architecture", "unowned", views),
    undefined,
  );
  assert.equal(c4NavigationTarget("docs/architecture/cli.c4", "", "index", views), undefined);
});

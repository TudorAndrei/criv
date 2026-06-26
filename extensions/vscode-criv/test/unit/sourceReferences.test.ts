import assert from "node:assert/strict";
import test from "node:test";

import {
  analyzeSourceReferences,
  appendSourceHoverContents,
  buildSourceTargetIndex,
  completionToken,
  sourceReferenceDiagnostic,
} from "../../src/sourceReferences";
import type { CrivStateSnapshot } from "../../src/stateModel";

const snapshot: CrivStateSnapshot = {
  raw: "{}",
  summary: {
    schema: "criv.state.v0",
    node_count: 2,
    edge_count: 0,
    source_count: 2,
    pattern_count: 0,
  },
  sources: [
    { path: "src/main.rs", frecency: 10 },
    { path: "docs/architecture/02-containers.c4", frecency: 1 },
  ],
  graphNodes: [
    {
      id: "symbol:src/main.rs#fn:run",
      kind: "function",
      label: "run",
      path: "src/main.rs#fn:run",
      source_target: "src/main.rs#fn:run",
      line_range: "L10-L20",
    },
  ],
  registeredPatterns: [],
  c4Artifacts: [],
};

test("links exact AST-aware selectors and criv source directives", () => {
  const index = buildSourceTargetIndex(snapshot);
  const references = analyzeSourceReferences(
    "See src/main.rs#fn:run\n%% criv:source src/main.rs",
    index,
  );

  assert.deepEqual(
    references.map((reference) => [reference.kind, reference.canonicalTarget]),
    [
      ["selector", "src/main.rs#fn:run"],
      ["criv-source", "src/main.rs"],
    ],
  );
});

test("warns for legacy source wikilinks and suggests canonical selectors", () => {
  const index = buildSourceTargetIndex(snapshot);
  const [reference] = analyzeSourceReferences("[[source:src/main.rs#run]]", index);

  assert.equal(reference?.kind, "typed-source-wikilink");
  assert.equal(reference?.canonicalTarget, "src/main.rs#fn:run");
  assert.equal(
    sourceReferenceDiagnostic(reference),
    "Legacy source target; use AST-aware source selector src/main.rs#fn:run.",
  );
});

test("reports unresolved criv source directives", () => {
  const index = buildSourceTargetIndex(snapshot);
  const [reference] = analyzeSourceReferences("%% criv:source src/missing.rs", index);

  assert.equal(reference?.kind, "criv-source");
  assert.equal(reference?.canonicalTarget, undefined);
  assert.equal(
    sourceReferenceDiagnostic(reference),
    "Unresolved criv source target: src/missing.rs.",
  );
});

test("extracts completion token at cursor offsets", () => {
  assert.deepEqual(completionToken("%% criv:source src/ma", 22), {
    query: "src/ma",
    start: 16,
  });
});

test("renders hover labels as text instead of trusted markdown", () => {
  const index = buildSourceTargetIndex({
    ...snapshot,
    graphNodes: [
      {
        id: "symbol:src/main.rs#fn:run",
        kind: "function",
        label: "[Run](command:workbench.action.terminal.sendSequence)",
        path: "src/main.rs#fn:run",
        source_target: "src/main.rs#fn:run",
        line_range: "L10-L20",
      },
    ],
  });
  const [reference] = analyzeSourceReferences("See src/main.rs#fn:run", index);
  const calls: Array<[string, string]> = [];

  appendSourceHoverContents(
    {
      appendMarkdown: (value) => calls.push(["markdown", value]),
      appendText: (value) => calls.push(["text", value]),
    },
    reference!,
  );

  assert(calls.some(([kind, value]) => kind === "text" && value.startsWith("[Run](")));
  assert(!calls.some(([kind, value]) => kind === "markdown" && value.includes("command:")));
});

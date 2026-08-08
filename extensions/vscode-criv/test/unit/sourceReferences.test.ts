import assert from "node:assert/strict";
import test from "node:test";

import {
  analyzeSourceReferences,
  appendSourceHoverContents,
  completionToken,
  sourceReferenceDiagnostic,
} from "../../src/sourceReferences";
import type { CrivGraphNode } from "../../src/wasm";

const sourceNode: CrivGraphNode = {
  id: "code:src/main.rs",
  kind: "code",
  label: "src/main.rs",
  path: "src/main.rs",
};
const runNode: CrivGraphNode = {
  id: "symbol:src/main.rs#fn:run",
  kind: "function",
  label: "run",
  path: "src/main.rs#L10-L20",
  source_target: "src/main.rs#fn:run",
  line_range: "L10-L20",
};
const lookup = (target: string): CrivGraphNode | undefined => {
  switch (target) {
    case "src/main.rs":
      return sourceNode;
    case "src/main.rs#fn:run":
    case "src/main.rs#run":
      return runNode;
    default:
      return undefined;
  }
};

test("links exact AST-aware selectors and criv source directives", () => {
  const references = analyzeSourceReferences(
    "See src/main.rs#fn:run\n%% criv:source src/main.rs",
    lookup,
  );

  assert.deepEqual(
    references.map((reference) => [reference.kind, reference.canonicalTarget]),
    [
      ["selector", "src/main.rs#fn:run"],
      ["criv-source", "src/main.rs"],
    ],
  );
});

test("warns for legacy source wikilinks and uses the canonical Wasm result", () => {
  const [reference] = analyzeSourceReferences("[[source:src/main.rs#run]]", lookup);

  assert.equal(reference?.kind, "typed-source-wikilink");
  assert.equal(reference?.canonicalTarget, "src/main.rs#fn:run");
  assert.equal(
    sourceReferenceDiagnostic(reference),
    "Legacy source target; use AST-aware source selector src/main.rs#fn:run.",
  );
});

test("reports unresolved criv source directives", () => {
  const [reference] = analyzeSourceReferences("%% criv:source src/missing.rs", lookup);

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
  const unsafeNode: CrivGraphNode = {
    ...runNode,
    label: "[Run](command:workbench.action.terminal.sendSequence)",
  };
  const [reference] = analyzeSourceReferences("See src/main.rs#fn:run", () => unsafeNode);
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

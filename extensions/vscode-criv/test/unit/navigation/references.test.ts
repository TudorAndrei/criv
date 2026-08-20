import assert from "node:assert/strict";
import test from "node:test";

import {
  ambiguousSourceTargetMessage,
  analyzeSourceReferences,
  appendSourceHoverContents,
  completionToken,
  planSourceTargetOpen,
  resolveSourceTarget,
  sourceReferenceDiagnosticCode,
  sourceReferenceDiagnostic,
} from "../../../src/navigation/references";
import type { CrivGraphNode } from "../../../src/state/wasm";

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
const lookup = (target: string) => {
  let node: CrivGraphNode | undefined;
  switch (target) {
    case "src/main.rs":
      node = sourceNode;
      break;
    case "src/main.rs#fn:run":
    case "src/main.rs#run":
      node = runNode;
      break;
    default:
      return { kind: "unresolved" as const };
  }
  return {
    kind: "resolved" as const,
    canonical_target: node.source_target ?? node.path!,
    node,
  };
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
  assert.equal(sourceReferenceDiagnosticCode(reference), "unresolved-source-target");
});

test("strips source wrappers and keeps valid line ranges out of identity", () => {
  const result = resolveSourceTarget(lookup, "source:src/main.rs#L10-L20");

  assert.equal(result.kind, "resolved");
  assert.equal(result.kind === "resolved" && result.canonical_target, "src/main.rs");
});

test("rejects malformed line syntax before lookup", () => {
  let calls = 0;
  const result = resolveSourceTarget(() => {
    calls += 1;
    return { kind: "unresolved" };
  }, "src/main.rs#Lx");

  assert.deepEqual(result, { kind: "malformed" });
  assert.equal(calls, 0);
});

test("opens only a resolved canonical path and keeps requested line navigation", () => {
  const resolved = planSourceTargetOpen(lookup, "source:src/main.rs#L12-L14");
  const unresolved = planSourceTargetOpen(() => ({ kind: "unresolved" }), "missing.rs");
  const ambiguous = planSourceTargetOpen(
    () => ({ kind: "ambiguous", candidates: [], total_candidate_count: 2 }),
    "main.rs",
  );

  assert.deepEqual(resolved, {
    kind: "resolved",
    target: { path: "src/main.rs", fragment: "L12-L14", line: 11, endLine: 13 },
  });
  assert.deepEqual(unresolved, { kind: "unresolved" });
  assert.deepEqual(ambiguous, {
    kind: "ambiguous",
    candidates: [],
    total_candidate_count: 2,
  });
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
  const [reference] = analyzeSourceReferences("See src/main.rs#fn:run", () => ({
    kind: "resolved",
    canonical_target: unsafeNode.source_target!,
    node: unsafeNode,
  }));
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

test("reports ambiguous targets with untrusted stable candidates", () => {
  const candidates = [
    {
      canonical_target: "src/a.rs#fn:run",
      node_id: "symbol:src/a.rs#fn:run",
      kind: "function",
      label: "[run](command:unsafe)",
    },
    {
      canonical_target: "src/a.rs#fn:run",
      node_id: "symbol:src/a.rs#method:run",
      kind: "method",
      label: "run",
    },
  ];
  const message = ambiguousSourceTargetMessage("src/a.rs#run", candidates, 3);

  assert.equal(
    message,
    "Ambiguous criv source target src/a.rs#run: src/a.rs#fn:run (function: [run](command:unsafe)) [symbol:src/a.rs#fn:run]; src/a.rs#fn:run (method: run) [symbol:src/a.rs#method:run]; 1 more.",
  );

  const [reference] = analyzeSourceReferences("[[source:src/a.rs#run]]", () => ({
    kind: "ambiguous",
    candidates,
    total_candidate_count: 3,
  }));
  assert.equal(reference?.resolutionKind, "ambiguous");
  assert.equal(sourceReferenceDiagnosticCode(reference!), "ambiguous-source-target");
});

test("navigates exact Elixir selectors and reports legacy arity ambiguity", () => {
  const runOne: CrivGraphNode = {
    id: "symbol:lib/sample.ex#module:My.App/fn:run/1",
    kind: "function",
    label: "run/1",
    path: "lib/sample.ex#L3-L3",
    source_target: "lib/sample.ex#module:My.App/fn:run/1",
    line_range: "L3-L3",
  };
  const runTwo: CrivGraphNode = {
    id: "symbol:lib/sample.ex#module:My.App/fn:run/2",
    kind: "function",
    label: "run/2",
    path: "lib/sample.ex#L4-L4",
    source_target: "lib/sample.ex#module:My.App/fn:run/2",
    line_range: "L4-L4",
  };
  const elixirLookup = (target: string) => {
    if (
      target === runOne.source_target ||
      target === "lib/sample.ex#run/1" ||
      target === "lib/sample.ex#My.App.run/1"
    ) {
      return {
        kind: "resolved" as const,
        canonical_target: runOne.source_target!,
        node: runOne,
      };
    }
    if (target === runTwo.source_target || target === "lib/sample.ex#run/2") {
      return {
        kind: "resolved" as const,
        canonical_target: runTwo.source_target!,
        node: runTwo,
      };
    }
    if (target === "lib/sample.ex#run") {
      return {
        kind: "ambiguous" as const,
        candidates: [
          {
            canonical_target: runOne.source_target!,
            node_id: runOne.id,
            kind: runOne.kind,
            label: runOne.label,
          },
          {
            canonical_target: runTwo.source_target!,
            node_id: runTwo.id,
            kind: runTwo.kind,
            label: runTwo.label,
          },
        ],
        total_candidate_count: 2,
      };
    }
    return { kind: "unresolved" as const };
  };

  const references = analyzeSourceReferences(
    "Exact lib/sample.ex#module:My.App/fn:run/1\nLegacy [[source:lib/sample.ex#run/1]]\nAmbiguous [[source:lib/sample.ex#run]]",
    elixirLookup,
  );
  assert.equal(references[0]?.canonicalTarget, runOne.source_target);
  assert.equal(references[1]?.canonicalTarget, runOne.source_target);
  assert.equal(
    sourceReferenceDiagnostic(references[1]!),
    `Legacy source target; use AST-aware source selector ${runOne.source_target}.`,
  );
  assert.equal(references[2]?.resolutionKind, "ambiguous");
  assert.equal(sourceReferenceDiagnosticCode(references[2]!), "ambiguous-source-target");

  assert.deepEqual(planSourceTargetOpen(elixirLookup, "lib/sample.ex#My.App.run/1"), {
    kind: "resolved",
    target: {
      path: "lib/sample.ex",
      fragment: "L3-L3",
      line: 2,
      endLine: 2,
    },
  });
});

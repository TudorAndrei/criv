import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { createPackagedCrivStateModule } from "@criv/editor-state/packaged";

import {
  analyzeSourceReferences,
  appendSourceHoverContents,
  completionToken,
  planSourceTargetOpen,
  sourceReferenceDiagnosticCode,
  sourceReferenceDiagnostic,
  type SourceReference,
  type SourceTargetLookup,
} from "../../../src/navigation/references";

interface StateFixture {
  state: { graph: { nodes: { id: string; label: string }[] } };
}

const fixtures = resolve(__dirname, "../../../../../fixtures");
const linkFixture = readFixture("link-resolution.json");
const lookupFixture = readFixture("editor/source-target-lookup.v1.json");

function readFixture(path: string): StateFixture {
  return JSON.parse(readFileSync(resolve(fixtures, path), "utf8")) as StateFixture;
}

async function loadedLookup(state: StateFixture["state"]): Promise<SourceTargetLookup> {
  const revision = await createPackagedCrivStateModule().loadState(JSON.stringify(state));
  return (target) => revision.lookupSourceTarget(target);
}

/** The link fixture State with one node label replaced. */
function linkStateWithLabel(id: string, label: string): StateFixture["state"] {
  const state = structuredClone(linkFixture.state);
  const node = state.graph.nodes.find((candidate) => candidate.id === id);
  assert.ok(node, `fixture node ${id}`);
  node.label = label;
  return state;
}

function canonical(reference: SourceReference | undefined): string | undefined {
  return reference?.resolution.kind === "resolved"
    ? reference.resolution.canonical_target
    : undefined;
}

function hoverCalls(reference: SourceReference): Array<[string, string]> {
  const calls: Array<[string, string]> = [];
  appendSourceHoverContents(
    {
      appendMarkdown: (value) => calls.push(["markdown", value]),
      appendText: (value) => calls.push(["text", value]),
    },
    reference,
  );
  return calls;
}

test("links resolved AST-aware selectors only", async () => {
  const lookup = await loadedLookup(linkFixture.state);

  const references = analyzeSourceReferences(
    "See src/lib.rs#fn:run and src/missing.rs#fn:gone",
    lookup,
  );

  assert.deepEqual(
    references.map((reference) => [reference.kind, canonical(reference)]),
    [["selector", "src/lib.rs#fn:run"]],
  );
  assert.equal(sourceReferenceDiagnostic(references[0]!), undefined);
});

test("warns for a source Wikilink with the criv check rule", async () => {
  const lookup = await loadedLookup(linkFixture.state);

  const references = analyzeSourceReferences("[[source:src/lib.rs#run]] [[src/lib.rs]]", lookup);

  assert.deepEqual(
    references.map((reference) => [
      reference.kind,
      sourceReferenceDiagnosticCode(reference),
      sourceReferenceDiagnostic(reference),
    ]),
    [
      [
        "wikilink",
        "source-wikilink",
        "Wikilinks are reserved for document references; use AST-aware source selector src/lib.rs#fn:run.",
      ],
      [
        "wikilink",
        "source-wikilink",
        "Wikilinks are reserved for document references; use AST-aware source selector src/lib.rs.",
      ],
    ],
  );
});

test("reports an unresolved source Wikilink", async () => {
  const [reference] = analyzeSourceReferences(
    "[[missing.rs]]",
    await loadedLookup(linkFixture.state),
  );

  assert.equal(sourceReferenceDiagnostic(reference!), "Unresolved criv target: missing.rs.");
  assert.equal(sourceReferenceDiagnosticCode(reference!), "unresolved-source-target");
});

test("opens only a resolved canonical path and keeps requested line navigation", async () => {
  const lookup = await loadedLookup(linkFixture.state);

  assert.deepEqual(planSourceTargetOpen(lookup, "source:src/lib.rs#L12-L14"), {
    kind: "resolved",
    path: "src/lib.rs",
    lines: { start: 12, end: 14 },
  });
  assert.deepEqual(planSourceTargetOpen(lookup, "src/lib.rs#fn:run"), {
    kind: "resolved",
    path: "src/lib.rs",
    lines: { start: 1, end: 1 },
  });
  assert.deepEqual(planSourceTargetOpen(lookup, "missing.rs"), { kind: "unresolved" });
  const ambiguous = planSourceTargetOpen(lookup, "shared.rs");
  assert.equal(ambiguous.kind, "ambiguous");
  assert.equal(ambiguous.kind === "ambiguous" && ambiguous.total_candidate_count, 2);
});

test("extracts completion token at cursor offsets", () => {
  assert.deepEqual(completionToken("See src/ma", 10), { query: "src/ma", start: 4 });
});

test("renders hover labels as text instead of trusted markdown", async () => {
  const lookup = await loadedLookup(
    linkStateWithLabel(
      "symbol:src/lib.rs#fn:run",
      "[Run](command:workbench.action.terminal.sendSequence)",
    ),
  );
  const [reference] = analyzeSourceReferences("See src/lib.rs#fn:run", lookup);

  const calls = hoverCalls(reference!);

  assert(calls.some(([kind, value]) => kind === "text" && value.startsWith("[Run](")));
  assert(!calls.some(([kind, value]) => kind === "markdown" && value.includes("command:")));
});

test("reports ambiguous targets with untrusted stable candidates", async () => {
  const lookup = await loadedLookup(
    linkStateWithLabel("code:src/one/shared.rs", "[run](command:unsafe)"),
  );
  const [reference] = analyzeSourceReferences("[[shared.rs]]", lookup);

  assert.equal(reference?.resolution.kind, "ambiguous");
  assert.equal(sourceReferenceDiagnosticCode(reference!), "ambiguous-source-target");
  const calls = hoverCalls(reference!);
  assert.deepEqual(calls, [["text", sourceReferenceDiagnostic(reference!)]]);
  assert.match(calls[0]![1], /\(code: \[run\]\(command:unsafe\)\)/);
});

test("navigates exact Elixir selectors and reports legacy arity ambiguity", async () => {
  const lookup = await loadedLookup(lookupFixture.state);
  const runOne = "lib/sample.ex#module:My.App/fn:run/1";

  const references = analyzeSourceReferences(
    `Exact ${runOne}\nLegacy [[source:lib/sample.ex#run/1]]\nAmbiguous [[source:lib/sample.ex#run]]`,
    lookup,
  );

  assert.equal(canonical(references[0]), runOne);
  assert.equal(canonical(references[1]), runOne);
  assert.equal(sourceReferenceDiagnosticCode(references[1]!), "source-wikilink");
  assert.equal(references[2]?.resolution.kind, "ambiguous");
  assert.equal(sourceReferenceDiagnosticCode(references[2]!), "ambiguous-source-target");
  assert.deepEqual(planSourceTargetOpen(lookup, "lib/sample.ex#My.App.run/1"), {
    kind: "resolved",
    path: "lib/sample.ex",
    lines: { start: 3, end: 3 },
  });
});

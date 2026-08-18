import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "../..");
const outFile = resolve(tmpdir(), `criv-preview-test-${process.pid}.mjs`);

mkdirSync(dirname(outFile), { recursive: true });
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/source/preview.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
});

const preview = await import(pathToFileURL(outFile).href);

assert.equal(preview.languageForPath("lib/sample.ex"), "elixir");
assert.equal(preview.languageForPath("test/sample_test.exs"), "elixir");
assert.equal(preview.languageForPath("src/lib.rs"), "rust");

const source =
  "defmodule Sample do\n  def run(value \\ 42), do: {:ok, ~r/foo\\/bar/i, nil, true} # safe <b>\nend";
const tokens = source.split("\n").flatMap((line) => preview.highlightSourceLine(line, "elixir"));
const classes = new Map(
  tokens.filter((token) => token.className).map((token) => [token.text, token.className]),
);

assert.equal(classes.get("defmodule"), "criv-token-keyword");
assert.equal(classes.get("def"), "criv-token-keyword");
assert.equal(classes.get("do"), "criv-token-keyword");
assert.equal(classes.get("end"), "criv-token-keyword");
assert.equal(classes.get(":ok"), "criv-token-literal");
assert.equal(classes.get("42"), "criv-token-number");
assert.equal(classes.get("nil"), "criv-token-literal");
assert.equal(classes.get("true"), "criv-token-literal");
assert.equal(classes.get("~r/foo\\/bar/i"), "criv-token-string");
assert.equal(classes.get("# safe <b>"), "criv-token-comment");

const unsafe = '<script>alert("x")</script>';
assert.equal(
  preview
    .highlightSourceLine(unsafe, "elixir")
    .map((token) => token.text)
    .join(""),
  unsafe,
  "the highlighter keeps source as text for safe DOM insertion",
);

const rust = preview.highlightSourceLine("fn run() { // comment", "rust");
assert.equal(rust.find((token) => token.text === "fn")?.className, "criv-token-keyword");
assert.equal(rust.find((token) => token.text === "// comment")?.className, "criv-token-comment");

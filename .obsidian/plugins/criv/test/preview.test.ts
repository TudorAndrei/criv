import assert from "node:assert/strict";
import test from "node:test";
import type { App } from "obsidian";

import type { LinkedSource } from "../src/source/model";
import { highlightSourceLine, languageForPath, readSourcePreview } from "../src/source/preview";

test("reads the linked lines of a source file", async () => {
  const app = {
    vault: { adapter: { read: async () => "one\ntwo\nthree\nfour" } },
  } as unknown as App;
  const linked = {
    entry: { path: "lib/sample.ex" },
    lines: { start: 2, end: 3 },
  } as LinkedSource;

  assert.deepEqual(await readSourcePreview(app, linked), {
    path: "lib/sample.ex",
    language: "elixir",
    text: "two\nthree",
    startLine: 2,
    truncated: false,
  });
});

test("names the preview language from the file extension", () => {
  assert.equal(languageForPath("lib/sample.ex"), "elixir");
  assert.equal(languageForPath("test/sample_test.exs"), "elixir");
  assert.equal(languageForPath("src/lib.rs"), "rust");
});

test("classifies Elixir tokens", () => {
  const source =
    "defmodule Sample do\n  def run(value \\ 42), do: {:ok, ~r/foo\\/bar/i, nil, true} # safe <b>\nend";
  const classes = new Map(
    source
      .split("\n")
      .flatMap((line) => highlightSourceLine(line, "elixir"))
      .filter((token) => token.className)
      .map((token) => [token.text, token.className]),
  );

  for (const [text, className] of [
    ["defmodule", "criv-token-keyword"],
    ["def", "criv-token-keyword"],
    ["do", "criv-token-keyword"],
    ["end", "criv-token-keyword"],
    [":ok", "criv-token-literal"],
    ["42", "criv-token-number"],
    ["nil", "criv-token-literal"],
    ["true", "criv-token-literal"],
    ["~r/foo\\/bar/i", "criv-token-string"],
    ["# safe <b>", "criv-token-comment"],
  ] as const) {
    assert.equal(classes.get(text), className, text);
  }
});

test("keeps source text intact for text-only DOM insertion", () => {
  const unsafe = '<script>alert("x")</script>';
  assert.equal(
    highlightSourceLine(unsafe, "elixir")
      .map((token) => token.text)
      .join(""),
    unsafe,
  );
  const rust = highlightSourceLine("fn run() { // comment", "rust");
  assert.equal(rust.find((token) => token.text === "fn")?.className, "criv-token-keyword");
  assert.equal(rust.find((token) => token.text === "// comment")?.className, "criv-token-comment");
});

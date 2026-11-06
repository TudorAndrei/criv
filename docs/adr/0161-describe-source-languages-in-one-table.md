---
id: ADR-0161
kind: decision
title: Describe Source Languages in One Table
status: accepted
date: 2026-11-06
governs:
  - src/source/graph.rs
  - src/source/graph/extract.rs
  - src/source/graph/syntax.rs
  - src/source/graph/elixir.rs
  - src/discovery/mod.rs
---

# Describe Source Languages in One Table

## Context

[[0129-own-elixir-graph-meaning-in-the-elixir-module|ADR-0129]] gives
`src/source/graph.rs` the extraction of Rust, TypeScript, JavaScript, Python,
and Go, and rejects a language trait.
[[0004-tree-sitter-source-graph|ADR-0004]] allows a conservative fallback
when parsing fails.

The knowledge about one language was spread over more than a dozen `match`
sites in `graph.rs` and `extract.rs`: grammar, symbol node kinds, module and
import forms, export and visibility rules, signature text, call keywords, and
media type. The extractor imported twenty private helpers from `graph.rs`.

The tree-sitter helpers had copies in the generic and Elixir code: two depth
limits, two `first_named_child`, two top-level splitters, two
matching-delimiter searches, and four `SourceFile` and several `Symbol` struct
literals. Five descendant searches had no depth limit, and Elixir call
arguments restarted the depth count at zero.

A file with a syntax error went to a line-based extractor. No test asserted
its output. Tree-sitter always returns a tree and marks invalid parts with
error nodes, so the line extractor only replaced a recovered tree with a
weaker guess.

The Source event filter of the watch built its scope with `.ok()?` and
treated every event as relevant when the configuration did not compile.

## Decision

`src/source/graph/extract.rs` owns every per-language fact in one table of
`LanguageSpec` rows: name, extensions, media type override, grammar, implicit
entry points, and, for the generic extractor, the node kinds of symbols,
modules, and imports, the parent container, extra call keywords, and the
export, visibility, and signature rules. `Language::as_str`, `mime`, and
`from_path` read the table. A table row is data. It is not the language trait
that ADR-0129 rejects, and Elixir meaning stays in the Elixir module.

`src/source/graph/syntax.rs` owns the shared tree-sitter and delimiter
helpers: one depth bound, the `Depth` counter of recursive walks, the
depth-bounded `descendants` iterator, `node_text`, `field_text`,
`first_named_child`, `node_range`, `matching_close`, `split_top_level`, and
`split_top_level_operator`. Every walk in the generic and Elixir extractors
uses them. `SourceFile::new` and `Symbol::new` are the only struct literals.

Remove the line-based extractor. A file with invalid syntax keeps the
declarations that tree-sitter recovers. This refines the fallback allowance
of ADR-0004. Bump the Source graph cache to `criv.source-graph/4`, because a
cached file that the line extractor parsed would otherwise be reused.

`src/source/graph.rs` keeps graph assembly, the cache, and queries.
`SourceGraph` fields are private. `resolved_edges` is the one place that
resolves the calls and relationships of a symbol; callers, callees, the
attack surface, and State projection use it. `file_fingerprint` exposes the
content hash of a parsed file.

`src/discovery/mod.rs` owns `SourceScope`: the enabled flag, the normalized
roots, and the exclusion and prune rules, built once from `Config` with a
`Result`. The candidate walk and the watch event filter read it. A bad
exclusion glob fails both.

## Consequences

A new language is one table row and, at most, one parse form. A change to a
shared helper has one place.

Source files with syntax errors now show tree-sitter declarations instead of
line guesses. The first refresh after the upgrade rebuilds the Source graph
cache.

`callers` no longer lists a symbol whose call resolves to a different symbol
with the same name. An unresolved call still matches by name.

A watch whose Source exclusions do not compile stops with an error instead of
refreshing on every event.

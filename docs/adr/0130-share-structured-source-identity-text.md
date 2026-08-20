---
id: ADR-0130
kind: decision
title: Share Structured Source Identity Text
status: accepted
date: 2026-08-21
governs:
  - src/source/graph.rs
  - src/state.rs
  - crates/criv-state-wire/src/**/*.rs
  - crates/criv-wasm/src/source.rs
  - fixtures/editor/source-target-lookup.v1.json
---

# Share Structured Source Identity Text

## Context

[[0088-share-the-state-wire-document|ADR-0088]] makes
`criv-state-wire` the shared Rust contract between native State publication
and Wasm State consumption. [[0091-enforce-editor-adapter-boundaries|ADR-0091]]
keeps source-target lookup in the active Wasm revision.
[[0099-enforce-shared-likec4-and-wasm-adapter-boundary|ADR-0099]] keeps editor
hosts from rebuilding State meaning.

Native Source graph code creates selector strings. Elixir selectors contain a
structured owner, optional callable kind, name, and arity. Their values use
uppercase percent encoding for every UTF-8 byte outside the URI unreserved
set. Wasm separately searches these strings for callable markers and
implements its own percent decoder so it can create the ADR-0119 compatibility
aliases.

The State wire still exposes only strings. A change to selector grammar or
escaping can therefore pass native tests while Wasm reads a different
identity. The shared type is missing even though both Rust consumers already
depend on one shared crate.

[[0129-own-elixir-graph-meaning-in-the-elixir-module|ADR-0129]] assigns
language meaning to the private Elixir graph implementation. That module must
construct the structured identity. It must not become a second owner for the
stable text grammar.

## Decision

Add a public `source_identity` module to `criv-state-wire`. This module is
the only Rust implementation of structured Source identity and stable Source
identity text conversion.

Keep the serialized State document unchanged. Graph node IDs, edge endpoints,
source targets, fixtures, Wasm results, and editor values remain strings. The
shared type is the Rust meaning at the producer and consumer edges; it is not a
new State row.

### Shared types

Define these shared values:

- `SourceIdentity` contains one repository-relative source path and either no
  selector for a file identity or one `SourceSelector` for a symbol identity.
- `SourceSelector::Opaque` keeps the exact selector text used by Rust,
  TypeScript, JavaScript, Python, Go, and any unknown future selector.
- `SourceSelector::Elixir` contains one structured `ElixirSelector`.
- `ElixirSelector` is either one owner identity or one callable identity.
- `ElixirOwner` is either a module name or a protocol and `for:` type pair.
- `ElixirCallableKind` is function, macro, guard, callback, or
  macro-callback.
- An Elixir callable identity contains its owner, callable kind, source name,
  and source arity.

Keep fields private. Expose constructors and read-only accessors for path,
selector, Elixir owner, callable kind, name, and arity. Do not expose percent
encoding or decoding as separate caller functions.

Use `Display` for canonical text and one parse operation for wire text. A file
identity formats as its path. A symbol identity formats as
`path#selector`. The parser splits the target exactly as current lookup does
and keeps a selector opaque unless the complete selector matches an ADR-0119
Elixir form.

A malformed or unknown selector remains opaque. It does not reject a complete
State revision. Exact lookup continues to use the published raw string. Wasm
uses structured Elixir data only to add compatibility aliases.

### Stable Elixir grammar

The shared implementation owns these canonical forms:

```text
module:My.App
module:My.App/fn:run/2
module:My.App/macro:build/1
module:My.App/guard:valid/1
module:My.Behaviour/callback:run/1
module:My.Behaviour/macro-callback:build/1
impl:Enumerable/for:My.App
impl:Enumerable/for:My.App/fn:reduce/3
```

Module-like kinds continue to use the `module:` owner form. Implementations
continue to use `impl:` and `for:`. Callable-kind prefixes and source
arities stay unchanged.

Percent-encode every byte outside ASCII letters, digits, hyphen, period,
underscore, and tilde. Use two uppercase hexadecimal digits. Decode a complete
percent-encoded value as UTF-8 only when parsing a recognized Elixir selector.
An incomplete escape, non-hexadecimal escape, invalid UTF-8 result, unknown
prefix, missing owner part, empty required value, or invalid arity makes the
selector opaque.

Parsing and then formatting native canonical text must return the same bytes.
Opaque selectors always format to their exact input bytes.

### Native ownership

The native Source graph uses the shared constructors for every Elixir owner
and callable selector. Remove its `selector_value` encoder and all manual
Elixir selector formatting.

The private Elixir graph module selected by ADR-0129 decides the structured
owner, callable kind, name, and arity. It passes those values to
`criv-state-wire` for text conversion.

The general graph can keep `SymbolId` and its current private cache fields.
Use the shared type at identity construction and display edges. Do not change
the serialized Source graph cache shape only to store the shared type. Keep
`criv.source-graph/3` and its cache recovery behavior.

Native State production continues to prefix the formatted identity with
`symbol:` or `code:`. It does not serialize the Rust type or add another
field.

### Wasm ownership

Wasm parses each prepared node source target once through the shared type. It
keeps exact lookup and canonical target output on the original State string.

For a parsed Elixir callable whose owner is a module, Wasm can create the
qualified compatibility alias from the decoded owner and the existing
callable label. For an implementation owner, it keeps the current unqualified
aliases only. Unknown or malformed selectors add no Elixir-specific alias.

Remove `elixir_callable_owner`, `decode_selector_value`, `hex_value`, and
manual callable-marker scanning from `criv-wasm`. Wasm still owns lookup
indexes, ambiguity, selector ranking, prepared revision lifetime, and editor
projection results.

Do not export the shared Rust type to TypeScript. Editor hosts continue to
receive only canonical strings and prepared Wasm results.

### Tests and enforcement

The shared crate owns table-driven contract tests for:

- every owner and callable form from ADR-0119;
- implementation owners and module owners;
- names with spaces, operators, percent signs, colons, slashes, and
  non-ASCII UTF-8;
- uppercase percent output and valid upper- and lower-case input escapes;
- empty values, invalid arities, incomplete escapes, invalid hexadecimal,
  invalid UTF-8, unknown forms, and opaque current-language selectors; and
- exact parse and format round trips.

Native tests prove that graph symbols and published State node IDs keep the
golden text. Wasm tests use the same editor fixture to prove exact and legacy
lookup, ambiguity, operators, UTF-8 module names, and implementation behavior.
Current-language selectors remain exact opaque values.

The shared module is the test surface for grammar and escaping. Native and
Wasm tests do not repeat encoder or decoder unit tests. Their tests cover only
construction and consumption at each interface.

`criv-state-wire` is the only crate that both Rust consumers already use.
Cargo dependency direction enforces this shared seam. Existing editor
policies keep TypeScript from adding lookup meaning. No new syntax policy is
needed for Rust helper names; removal and interface tests prove the
implementation.

### Compatibility

Preserve:

- `criv.state.v1` and every serialized State row;
- every graph node ID, edge endpoint, source target, selector, and fixture
  string;
- exact, unique legacy, missing, and ambiguous lookup results;
- selector suggestion text, order, scores, and limits;
- Wasm validation and one-loaded-revision behavior;
- editor host roles, JSON results, navigation, and error behavior;
- `criv.source-graph/3`, cache reuse, and cache recovery; and
- all current-language Source graph behavior.

This decision changes Rust ownership and types only. It does not authorize a
wire, selector, cache, editor, or lookup behavior change.

## Migration

1. Add `crates/criv-state-wire/src/source_identity.rs` and expose the shared
   types from the crate interface. Add complete grammar, escaping, malformed
   input, and opaque-selector contract tests there.
2. Replace native Elixir owner and callable string construction with shared
   structured values and canonical formatting. Keep the graph cache shape and
   all golden selector tests unchanged.
3. Use `SourceIdentity` when native State code creates symbol and code node
   identity text. Compare the complete State fixture before and after.
4. Parse prepared Wasm source targets through the shared type. Use structured
   Elixir owners for compatibility aliases, while exact lookup keeps the raw
   published string.
5. Delete Wasm marker scanning and percent decoding. Keep lookup, ambiguity,
   ranking, and result production in Wasm.
6. Run shared-crate, native graph, State, Wasm, editor-contract, and editor-host
   tests. Compare all checked-in State and editor fixtures.
7. Implement ADR-0129 after this migration. Move structured Elixir identity
   construction into the private Elixir child without moving text conversion
   out of the shared crate.
8. Update the Code architecture map from the implementation. Show native
   Source graph and Wasm Source projection using the shared Source identity
   implementation in `criv-state-wire`. Keep editor hosts outside this
   meaning.

Do not add a compatibility wrapper around the old encoder or decoder. Replace
both copies and delete them.

## Consequences

Native production and Wasm consumption use one grammar and one percent
implementation. A selector change has one implementation, one contract test
table, and two small caller integrations.

The shared State wire crate gains stable identity-text meaning because that
text is already part of its graph node and edge contract. It does not gain
Source parsing, lookup, State projection, file access, or editor behavior.

Deleting the shared Source identity module would put encoding back in native
code and decoding back in Wasm. The module therefore passes the deletion test
and provides leverage to both consumers.

## Alternatives Considered

### Keep a string and share only percent functions

Rejected. Callers would still parse owner, kind, name, arity, and selector
shape separately. Two small functions would not own the identity grammar.

### Put the type in the native Source module

Rejected. Wasm cannot depend on the CLI crate. Copying the type would preserve
the current drift.

### Put the type in the Wasm crate

Rejected. Native State production would then depend on an editor projection
crate or keep its own encoder.

### Add structured fields to State

Rejected. The current strings are sufficient when both Rust sides share their
meaning. New fields would change State v1 and editor data without adding
required behavior.

### Parse every current-language selector into one grammar

Rejected. Current selector values were not designed as one escaped grammar.
Treating them as opaque preserves their exact text and avoids inventing new
language rules.

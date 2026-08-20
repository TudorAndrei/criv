---
id: ADR-0129
kind: decision
title: Own Elixir Graph Meaning In The Elixir Module
status: accepted
date: 2026-08-21
governs:
  - src/source/graph.rs
  - src/source/graph/elixir.rs
---

# Own Elixir Graph Meaning In The Elixir Module

## Context

[[0119-first-class-elixir-language-support|ADR-0119]] defines Elixir modules,
callables, directives, relationships, selectors, ambiguity, and partial-parse
behavior. [[0105-owner-scoped-rust-module-layout|ADR-0105]] makes
`src/source/graph.rs` the private general graph implementation below the
Source interface.

The Elixir parser is already in `src/source/graph/elixir.rs`. The general
graph module still owns Elixir selector construction, compatibility aliases,
callable identity matching, lexical directive activation, alias expansion,
import filtering, relationship indexes, target resolution, and target labels.
One Elixir rule therefore needs changes in two large files. The general graph
module also needs to know Elixir callable kinds, directive precedence, and
module naming rules.

The two confirmed parser defects were fixed before this decision. UTF-8
specification text no longer causes invalid slicing, and effective default
arities come from syntax nodes. The ownership move must preserve those fixes.

Native State production creates stable selector text, while Wasm lookup parses
the same text. Issue 160 decides the shared Source identity type and text
conversion. Elixir ownership and shared identity are related, but they are
different meanings: Elixir owns which identity a declaration has; shared
Source identity owns how that identity becomes stable wire text.

## Decision

Keep `src/source/graph.rs` as the language-neutral Source graph
implementation. It owns:

- selected-file graph assembly and deterministic file order;
- cache loading, reuse, publication, and schema changes;
- shared graph storage types and indexes that apply to every language;
- language selection and the Tree-sitter entry point;
- generic symbol lookup, caller and callee traversal, interface hashes, and
  State-facing graph queries; and
- current Rust, TypeScript, JavaScript, Python, and Go extraction.

Deepen the private `src/source/graph/elixir.rs` module. It owns all rules
whose answer can differ because the language is Elixir:

- partial Tree-sitter parsing and declaration recovery;
- module, implementation, callable, directive, and relationship meaning;
- effective arities, clause merging, signatures, and visibility;
- structured Elixir owner and callable identities;
- Elixir compatibility aliases and ambiguity candidates;
- lexical directive scope and precedence;
- static module and alias resolution;
- import `only:` and `except:` filters and callable-kind filters;
- module and callable relationship indexes;
- relationship target resolution and target labels; and
- every Elixir-specific callable-kind or selector-prefix rule.

The general graph module can store shared data types that the Elixir
implementation fills. Storage ownership does not give the general module
language meaning. Do not move generic graph types into the Elixir child only
to hide them, and do not make the child public.

### Private interface

Use a small private interface between the general graph and the Elixir
implementation:

- one parse operation accepts a path, source text, and Tree-sitter root and
  returns a complete `SourceFile`;
- one compatibility-alias operation accepts an Elixir symbol and returns its
  exact lookup aliases; and
- one `ElixirRelationships` value builds all Elixir module and callable
  indexes from completed files, resolves one relationship from its caller
  context, and produces its target label.

`SourceGraph` stores `ElixirRelationships` as derived, non-serialized
state. Build it after a cold parse and rebuild it after cache restore. Generic
query traversal delegates one Elixir relationship to this value. The caller
does not inspect module indexes, directive history, alias mappings, import
filters, or candidate lists.

Do not add a language trait or a registry of language adapters. Only Elixir
needs this complete semantic implementation. A trait with one adapter would
add an interface without a second implementation.

### Shared Source identity

The Elixir module constructs structured Elixir owner and callable identity. It
does not own percent encoding, decoding, or the published text grammar after
the shared Source identity decision is implemented.

The shared Source identity module owns stable text conversion for native and
Wasm callers. The Elixir implementation supplies the structured identity and
asks that module for canonical selector text. It also uses the shared parsed
identity when a lookup needs language-specific compatibility aliases.

Keep the selector text from ADR-0119 byte-for-byte unchanged. Do not publish a
new State field, schema, or editor wire value to express this ownership.

### Tests and enforcement

Test Elixir behavior through the private Elixir interface and through complete
Source graph outcomes:

- focused Elixir tests cover declarations, specifications, default arities,
  structured identities, compatibility aliases, lexical aliases, imports,
  filters, relationship resolution, labels, parse recovery, and the two fixed
  correctness cases;
- multi-file Source graph tests cover exact and ambiguous lookup, callers,
  callees, cache restore, and stable selector text; and
- language-neutral Source graph tests keep current-language selectors and
  queries unchanged.

Move focused tests with the meaning they verify. Remove duplicate parent tests
after equivalent child-interface or Source-graph coverage exists. Do not test
private helper steps.

Rust module privacy keeps callers outside `source::graph` from importing the
Elixir child. The Code architecture map records the private implementation
below Source graph. A syntax policy cannot reliably distinguish an Elixir
semantic rule from language-neutral graph code, so this decision does not add
a broad name-based policy. Review and tests enforce the semantic split.

### Compatibility

Preserve:

- all ADR-0119 language behavior and exclusions;
- canonical selector text and compatibility alias results;
- exact, missing, and ambiguous lookup behavior;
- caller and callee results and relationship labels;
- State v1, Source graph cache data, editor wire data, and editor behavior;
- partial-file recovery and no lexical fallback for Elixir;
- deterministic file, symbol, and relationship order;
- cache reuse and relationship-index rebuild behavior; and
- Rust, TypeScript, JavaScript, Python, and Go behavior.

The implementation can change the in-memory owner of derived relationship
indexes. It must not change the serialized graph cache unless the shared
identity implementation needs a deliberate schema bump.

## Migration

1. Complete the shared Source identity decision and its implementation. Keep
   stable text conversion in that shared module before moving selector
   construction.
2. Add focused contract tests at the Elixir interface for selector inputs,
   compatibility aliases, directive scope, import filters, relationship
   resolution, target labels, Unicode specifications, and syntax-based
   defaults.
3. Move Elixir structured identity construction, selector-prefix choices, and
   compatibility alias rules into the Elixir child. Use the shared identity
   text converter. Delete the old general-graph implementations without
   forwarding functions.
4. Move the current module and callable relationship indexes into
   `ElixirRelationships`. Build and rebuild this derived value through the
   Elixir interface.
5. Move directive activation, alias expansion, import selection, filter
   evaluation, callable-kind matching, relationship resolution, and label
   construction into the Elixir child. Make generic query traversal delegate
   through the small interface.
6. Move focused Elixir tests beside the implementation. Keep only complete
   graph and current-language contract tests in the general graph module.
7. Update the Code architecture map from the migrated code. Show
   `criv::source::graph::elixir` as a private implementation below Source
   graph, and show the Source identity dependency selected by the shared
   identity decision.
8. Run workspace tests, Clippy, vault refresh and check, architecture
   validation, rendered-view inspection, and supported-platform validation.

Do not mix the ownership move with a change to Elixir behavior. If migration
reveals a semantic defect, fix it in a separate scoped commit with a focused
contract test.

## Consequences

The general Source graph no longer needs Elixir alias, import, selector, or
relationship rules. One Elixir change becomes local to one private module and
its interface tests. Generic graph queries keep one language-neutral
responsibility.

Deleting the Elixir module would move parser recovery, declaration meaning,
identity construction, directive scope, import filters, and relationship
resolution back into the general graph. The module therefore passes the
deletion test and gains more depth.

The private interface has one in-process implementation and no adapter. The
move changes locality and internal ownership, not public behavior.

## Alternatives Considered

### Keep relationship resolution in the general graph

Rejected. The general graph would still own Elixir lexical scope, alias
precedence, import filters, and callable-kind rules. Parser changes would
continue to cross both modules.

### Create a trait for every Source language

Rejected. Current languages use the shared generic extractor and do not need
separate semantic adapters. A trait would add a shallow interface and force
unrelated languages into the Elixir design.

### Move stable selector encoding into the Elixir module

Rejected. Native and Wasm must use one stable text conversion. Elixir owns the
structured language identity, while the shared Source identity module owns its
wire text.

### Move all graph types into the Elixir module

Rejected. Shared files, symbols, ranges, calls, and query results are
language-neutral storage. Moving them would make the general graph depend on
Elixir for common data without improving locality.

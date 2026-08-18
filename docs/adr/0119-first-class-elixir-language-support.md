---
id: ADR-0119
kind: decision
title: First class Elixir language support
status: accepted
date: 2026-08-18
supersedes:
  - ADR-0004
  - ADR-0097
governs:
  - Cargo.toml
  - Cargo.lock
  - src/source/graph.rs
  - src/structural.rs
  - src/state.rs
  - src/query.rs
  - crates/criv-wasm/src/source.rs
  - fixtures/editor/source-target-lookup.v1.json
  - .obsidian/plugins/criv/src/source/preview.ts
  - extensions/vscode-criv/src/navigation/references.ts
  - extensions/vscode-criv/src/navigation/target.ts
---

# First Class Elixir Language Support

## Context

[[0004-tree-sitter-source-graph|ADR-0004]] gives the Source graph grammar-backed
support for Rust, TypeScript, JavaScript, Python, and Go. Elixir files are now
selected as text, but the graph classifies them as an unknown language. The
result has file paths but no Elixir modules, callables, relationships, or
structural policy support.

Elixir does not map cleanly to the current class and method model. Its stable
callable identity is module, callable kind, name, and arity. A source file can
contain more than one module. Functions can have several ordered clauses and
default arguments can create more than one effective arity. Protocols,
implementations, behaviours, macros, guards, structs, and exceptions also have
language meanings that a class approximation would hide.

The Tree-sitter Elixir grammar represents declarations as calls whose target
names identify `defmodule`, `def`, and the other declaration forms. The grammar
can recover valid trees around `ERROR` and `MISSING` nodes. The current
`ast-grep-language` dependency also supports Elixir when its
`tree-sitter-elixir` feature is enabled.

[[0034-ast-aware-source-selectors|ADR-0034]] requires semantic, readable Source
selectors. [[0097-explicit-source-target-lookup-results|ADR-0097]] limits legacy
aliases to the five-language selector model. First class Elixir support needs
arity-aware identities and two new readable compatibility aliases.

## Decision

### Support boundary

Include Elixir in the default criv binary. Read and parse every selected `.ex`
and `.exs` text file from configured Source roots. Do not require a Mix project
and do not run Mix. Support syntax accepted by the included Tree-sitter grammar
without claiming a separate Elixir compiler-version range.

Keep HEEx, EEx, Mix-based discovery, framework-specific analysis, linting,
formatting, BEAM inspection, compiler macro expansion, named types, and general
module-body execution analysis outside this decision. In particular, do not
model `@type`, `@typep`, `@opaque`, `@typedoc`, `@impl`, or `defoverridable`.

### Module-like symbols

Create one module-like symbol for every statically named `defmodule`,
`defprotocol`, and `defimpl`. A module with `defstruct` has kind `struct`. A
module with `defexception` has kind `exception`. A module with `@callback` or
`@macrocallback` has kind `behaviour`. `defprotocol` has kind `protocol`.

A `defimpl` symbol has kind `implementation` and identity from its protocol and
`for:` type. It has separate links to both targets. Each `@behaviour Target`
adds a behaviour-implementation link from the current module to the target.

Resolve literal nested aliases and static `__MODULE__.Child` names. Keep each
same-named source declaration in a different file as a separate symbol. Do not
merge its ranges or body with another file. A lookup that cannot select one
declaration is ambiguous. Skip a dynamic module name instead of making an
uncertain symbol.

Module kinds are explicit graph concepts. Do not map them to class or method,
and do not leave them only in the private `ModuleDecl` list. Publish containment
from each module-like symbol to its child symbols.

### Callables and interfaces

Use these callable kinds:

- `def` and `defp` are functions.
- `defmacro` and `defmacrop` are macros.
- `defguard` and `defguardp` are guards.
- `defdelegate` is a function with delegation data.
- `@callback` is a callback contract.
- `@macrocallback` is a macro-callback contract.

Canonical callable identity is owner, kind, name, and source arity. Combine all
ordered clauses with the same identity into one symbol. Keep all clause ranges,
use the first clause as the State navigation target, and combine calls from all
clauses.

Create one symbol for each effective arity that an Elixir default argument
generates. Do not create a symbol for a bodyless default head. Keep fixed
operator names and zero-arity definitions in the same identity model. Skip a
definition whose static name or arity is not available.

`def`, `defmacro`, `defguard`, and `defdelegate` are public. Their `p` forms are
private. Callback declarations are public contracts and are not executable
calls. Mark callbacks named by `@optional_callbacks` as optional. Keep
macro-callback source names and arities; do not use the compiler `MACRO-` name
or its added runtime argument.

An interface signature contains callable kind, qualified name and arity,
visibility, ordered parameter patterns, guards, defaults, and every matching
`@spec` in source order. It excludes bodies. A specification does not create a
second identity. Ignore a specification without a matching static definition.

### Directives and relationships

Normalize Elixir module aliases without the `Elixir.` atom prefix. Keep Erlang
module atoms such as `:lists`. Resolve literal aliases, lexical aliases, brace
expansions, and static `__MODULE__` paths. Keep computed module names
unresolved.

Keep directive kind, lexical scope, `as:`, and static `only:` or `except:` data
in the native graph. `require ... as:` also creates an alias. Publish `alias`,
`import`, `require`, and `use` through the existing State `imports` edge. A
`use` relationship names only its direct target and does not infer injected
code.

Resolve a local call to an exact same-module callable first, then to one exact
explicit import after its filters. Keep zero or several candidates unresolved.
Never choose the first global short-name match. Resolve a remote call only when
its module is static. Count written arguments for arity, and add the pipeline
left value as the first argument.

Keep calls, captures, delegates, protocol implementations, and behaviour
implementations as distinct native relationships. A direct capture creates a
capture link, not a call link. A placeholder capture has no target. A delegate
keeps its source name and effective arities, links to a static `to:` and `as:`
target, and appears in caller and callee queries without a duplicate call edge.

An anonymous-function call has a call-site-specific dynamic target with known
arity. Resolve `apply/3` only when module, function atom, and argument-list
length are static. Other `apply/3` calls remain dynamic. Never merge unrelated
dynamic calls into one target.

Include calls from clause bodies, guards, and default values. Exclude typespec
expressions, callback type expressions, quoted code, declarations, directives,
and Elixir special forms. Do not infer calls from `unquote`, general module-body
expressions, macro output, or a built-in Kernel export catalogue. An explicit
`Kernel.name/arity` call can resolve normally.

### Selectors, State, and editors

Use stable owner selectors and these callable parts:

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

A module keeps its `module:` selector when its graph kind is struct, exception,
protocol, or behaviour. Store a structured owner identity instead of relying
on one short parent string. Percent-encode every UTF-8 byte outside the URI
unreserved set in selector values. Labels show normal Elixir text: a full
module name, `name/arity` for a callable, and `Protocol for Type` for an
implementation.

Resolve exact canonical selectors first. Keep `path#name` only when it is
unique. Add `path#name/arity` and `path#Module.name/arity` as unique Elixir
compatibility aliases. Report ambiguity across owners, kinds, or arities. Keep
all current-language canonical selectors and query rows unchanged.

Change the native Source graph cache to version 3. Keep `criv.state.v1`; its
node kinds, edge kinds, identities, and labels are open strings. General kind
queries can return the new symbol kinds. Caller and callee queries count
delegates but not captures. Keep `coverage --by module` as directory grouping.

Map `.ex` and `.exs` to editor language `elixir` and MIME `text/x-elixir`.
Keep Wasm as the canonical editor lookup owner and keep the current State and
editor wire schemas. Add Elixir comments, keywords, literals, atoms, and sigils
to the Obsidian Source preview.

### Errors, policies, and evidence

Use the partial Tree-sitter result for an Elixir file with parse errors. Keep a
declaration only when its complete declaration subtree has no `ERROR` or
`MISSING` node. Safe declarations before and after the error remain available.
Keep the file indexed. Never use the lexical fallback for Elixir.

Enable the `tree-sitter-elixir` feature in `ast-grep-language`. Accept
`language: elixir` and `language: ex`. Patterns, rules, `check`, `search`, and
`enforce` use the same parser and result contract. Invalid patterns and partial
files return diagnostics and never panic.

Use focused parser fixtures plus one multi-file end-to-end Elixir fixture.
Cover module kinds, callables, clauses, default arities, visibility,
relationships, selectors, errors, policies, cache reuse, State, queries, Wasm,
both editors, and current-language non-regression. All supported release
platforms must pass. There is no Elixir-specific throughput gate: correctness
requires criv to read and parse every selected `.ex` and `.exs` text file.

## Consequences

Elixir becomes a first class Source and policy language instead of an unknown
text extension. The native graph becomes more expressive, but current-language
identities and State v1 remain stable.

The grammar and explicit module model increase the default binary and add
implementation and test work across the CLI, Wasm, and editors. A separate
release decision owns the bounded dependency exception and the one-time
production baseline reset.

## Alternatives Considered

### Map Elixir modules to classes and callables to methods

Rejected. It hides Elixir concepts, loses arity identity, and creates selectors
that are difficult to correct later.

### Keep modules only in ModuleDecl

Rejected. That private list is not published to State or used by public
queries, so the result would not be first class support.

### Require Mix or compiler expansion

Rejected. criv works from configured Source roots and performs static source
analysis. Requiring a toolchain or executing macros would change that boundary.

### Skip or sample Elixir files for performance

Rejected. Language support requires every selected text file to be read and
parsed.

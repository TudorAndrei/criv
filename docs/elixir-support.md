---
id: elixir-support
kind: doc
title: Elixir source support
targets:
  symbols:
    - src/source/graph/elixir.rs
---

# Elixir source support

criv reads `.ex` and `.exs` files that are below the configured Source roots.
The criv binary contains the Elixir Tree-sitter grammar. It does not start
Elixir, Mix, or the BEAM. It does not read a Mix project to find more roots.

Files with `.eex` and `.heex` extensions are not Elixir source files. criv can
select them as text when they are below a Source root, but it does not create
Elixir symbols from them.

## Source graph

The Elixir adapter records these module forms:

- modules, protocols, and protocol implementations;
- structs and exceptions;
- behaviours and their function or macro callbacks.

It records public and private functions, macros, guards, delegates, clauses,
default arities, guards, and specifications. One named callable uses its
module, kind, name, and arity as its identity. For example:

```text
lib/my_app.ex#module:My.App
lib/my_app.ex#module:My.App/fn:run/2
lib/my_app.ex#module:My.App/macro:build/1
lib/my_app.ex#module:My.App/guard:valid/1
lib/protocol.ex#impl:Enumerable/for:My.App/fn:reduce/3
```

Several clauses with the same name and arity have one identity. A default
argument can create more than one arity.

Old short targets continue to work when they identify one symbol:

```text
lib/my_app.ex#run
lib/my_app.ex#run/2
lib/my_app.ex#My.App.run/2
```

criv does not select one result when a short target has two valid meanings.
Use the complete canonical selector in that case.

## Directives and calls

criv records `alias`, `import`, `require`, and `use` directives. It resolves
literal aliases, `__MODULE__`, static imports, remote calls, local calls,
pipelines, named captures, literal `apply/3` calls, and literal delegates. A
pipeline adds its left value to the target arity.

Some Elixir behavior is available only after code runs or a macro expands.
criv records these sites as dynamic when it cannot get one static target. This
includes function-value calls, computed module names, general `apply/3` calls,
and code that a `use` macro injects.

If one syntax region is invalid, criv keeps safe sibling declarations. It does
not claim symbols from an unsafe region.

## Policy and editor names

Use `language: elixir` or `language: ex` in an ADR policy. Both names scan
`.ex` and `.exs` files. State uses `text/x-elixir` as the MIME value, and both
editors use `elixir` as the language name.

Elixir source support does not include EEx or HEEx analysis, framework
analysis, Mix discovery, macro expansion, BEAM inspection, formatting, or
linting. It also does not model named type declarations or general module-body
execution.

The complete behavior is in
[[0119-first-class-elixir-language-support|ADR-0119]]. Release evidence and the
default grammar dependency are in
[[0120-reset-release-baseline-for-default-elixir-support|ADR-0120]].

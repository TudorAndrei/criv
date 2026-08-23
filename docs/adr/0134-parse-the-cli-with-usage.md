---
id: ADR-0134
kind: decision
title: Parse the CLI with Usage
status: accepted
date: 2026-08-23
supersedes:
  - ADR-0019
governs:
  - Cargo.toml
  - src/lib.rs
  - src/main.rs
  - README.md
---

# Parse the CLI with Usage

## Context

[[0003-adopt-proven-foundation-crates|ADR-0003]] chose Clap as the command
parser. [[0019-export-usage-cli-spec|ADR-0019]] then added `usage-lib`, kept
Clap as the parser, and gave Usage the help page and the exported KDL
specification. The specification came from the Clap command through a bridge,
and `src/lib.rs` corrected the rendered help afterwards with four helper
functions.

That arrangement needed two command models for one command tree. It also needed
about 40 lines of string rewriting to make the Usage page look like a Clap page.

`usage-rs` 6.0.0 went to crates.io on 2026-08-22. It is a typed parser with its
own derive macros, and it emits the KDL specification from the same
declaration. One model can now do the work that two did.

criv also runs in other people's repositories.
[[0001-local-cli-vault-architecture|ADR-0001]] makes the CLI the public
interface for humans, agents, hooks, and downstream tooling. An agent working in
another repository has the installed binary and nothing else. Files in this
repository, such as `docs/query-reference.md`, ship with the criv source and not
with the tool, so they document the query surface only for people who work on
criv itself.

## Decision

Parse the command line with `usage-rs`. Remove Clap from criv's direct
dependencies. The declaration keeps the same shape: one root type, three
subcommand enums, 16 option structs, and six value enums.

Declare `bin = "criv"`, `version`, and `unknown_flags = "error"` on the root.
Usage passes an unknown flag to the positional arguments by default, and it
inherits the setting down the tree, so one declaration restores refusal for
every command.

Keep the exit codes exactly. A usage failure exits 2, and a successful command
exits 0. Git hooks and CI read those codes.

Print the page that Usage renders. Do not correct it. The section word is
`Flags:`, the placeholder is `[FLAGS]`, and a required flag prints as
`criv enforce <--stage <STAGE>>`.

A bare `criv` prints the short root page and exits 0. A command that needs a
subcommand, such as `criv query`, writes its own page to standard error and
exits 2. That page lists every subcommand, so the hand-written hint that listed
valid `query` names is deleted.

Print the exported specification from `CrivCli::to_kdl()` behind the hidden
`--usage` flag. Keep `usage-lib` as a development dependency only, so one test
proves that the emitted document still parses with the consumer that
`README.md` names.

Make long help the source of truth for criv documentation. Reference prose
belongs in the command declaration, where every project can read it, and not in
a Markdown file that ships with the source. `docs/query-reference.md` becomes a
short note that points at `criv query --help` and keeps its `targets` entry for
`src/query.rs`.

Keep the external specification pipelines in `README.md` for completions,
Markdown, and manpages. Do not enable the `completions` feature and do not
depend on the `usage` binary at runtime, which
[[0019-export-usage-cli-spec|ADR-0019]] also refused. State the version rule
beside the pipelines: the `usage` CLI that reads the specification must be the
same major version as the `usage-rs` that criv builds with, because the
specification dialect changes between major versions.

This decision narrows two accepted decisions without replacing them.
[[0003-adopt-proven-foundation-crates|ADR-0003]] keeps its other crate choices,
but Clap is no longer one of criv's foundation crates.
[[0064-typed-query-subcommands|ADR-0064]] keeps its decision that every query is
a typed subcommand variant with typed value enums; read its words "Clap-derived"
and "Clap value enums" as the Usage equivalents.

## Consequences

Users see four changes. The help page says `Flags:` where it said `Options:`. A
required flag prints inside angle brackets. A bare `criv` prints the short page
instead of the long one. A wrong `query` name reports the wrong name without the
old list of valid names.

criv is before version 1.0, so a breaking change to the command-line surface is
acceptable when a decision records it. This decision records it.

The dependency graph falls from 180 crates to 152, and the release binary falls
from 12,168,992 bytes to 11,735,840 bytes, a fall of 433,152 bytes or 3.6 per
cent. Clap does not leave the build: `rumdl` requires `clap` and `clap_complete` with no feature gate, so 12
clap-family crates remain. This decision removes Clap from the criv CLI, not
from the dependency tree.

Two relationship attributes change spelling. Usage refuses `conflicts_with` and
asks for `conflicts = "--fix"`, which names the flag as the specification spells
it. `requires` keeps its name.

A default value uses the portable `default = "text"` spelling. Usage refuses a
bare `default_value_t`, because the portable literal must also reach the
specification.

The four help correction helpers, the Clap error inspection, and the
hand-written `query` hint are deleted from `src/lib.rs`.

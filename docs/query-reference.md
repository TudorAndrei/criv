---
id: query-reference
kind: doc
title: criv query reference
targets:
  symbols:
    - src/query.rs
---

# criv query reference

The CLI is the reference for `criv query`. Run these commands:

```sh
criv query --help
criv query <subcommand> --help
```

The help pages hold every subcommand, positional argument, flag, choice, and
default. They come from the command tree in `src/query.rs`, so they cannot
drift from the code.

Every query takes `--format text`, `--format json`, or `--format ndjson`, and
`--limit <N>` to bound the answer before it is printed.

## Why the CLI and not this file

criv runs in other people's repositories. An agent working there has the
installed binary and nothing else, because this file ships with the criv source
and not with the tool. Documentation that only criv developers can read is not
documentation of the tool.

Long help carries the detail. `-h` prints the short page, and `--help` prints
the full one.

## Generated reference material

To make shell completions, a Markdown page, or a manpage from the same command
tree, pipe the exported spec into the `usage` CLI:

```sh
criv --usage | usage generate completion --file - zsh criv
criv --usage | usage generate markdown --file - --out-file docs/cli.md
criv --usage | usage generate manpage --file - --out-file criv.1
```

The `usage` CLI must have the same major version as the `usage-rs` crate that
criv builds with. The spec dialect changes between major versions, so an older
reader cannot parse a newer spec.

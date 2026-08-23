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

To make a Markdown page or a manpage from the same command tree, pipe the
exported spec into the `usage` CLI. `README.md` holds those commands and the
version rule they need.

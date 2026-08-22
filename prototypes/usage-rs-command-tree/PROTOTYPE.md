# Prototype: the criv command tree in usage-rs 6

PROTOTYPE. Throwaway. It answers wayfinder ticket
[#189](https://github.com/TudorAndrei/criv/issues/189). It is not migration code.

Date: 2026-08-22. Built with `usage-rs` 6.0.0 and `rustc` 1.97.1.

## What it declares

The criv root command, plus the two subcommands that carry the hard cases:

- `check`, with `conflicts_with` and a default value.
- `enforce`, with a required enum flag and two hidden `requires` flags.

Run it with `cargo run --bin criv-proto -- <args>` in this directory.

## Result

The declaration reads well. It is a mechanical rewrite of the clap declaration.
The output is close to clap. Four facts came out of the build that the research
tickets did not have.

## Correction 1: `default_value_t` alone does not compile

The audit in [#186](https://github.com/TudorAndrei/criv/issues/186) says
`default_value_t` maps with no difference. The compiler disagrees:

```text
error: `default_value_t` needs exactly one `default = "..."` beside it: the Rust
expression supplies the runtime value and the literal is emitted into the
portable spec
```

A second try with `default_value_t = Format::Text, default = "text"` also fails,
because usage calls `ToString` on the expression and clap's `ValueEnum` derive
gives no `Display`:

```text
error[E0277]: the trait bound `Format: ToString` is not satisfied
```

The spelling that works is the portable one alone:

```rust
#[usage(long, value_enum, default = "text")]
format: Format,
```

criv has two `default_value_t` sites, in `src/check.rs` and `src/query.rs`. Both
must change to `default = "…"`, or both enums need a `Display` implementation.

## Correction 2: the exported KDL is not quoted

The audit says the KDL text changes, and gives `flag "--usage" hide=#true` and
`bin "criv"`. The real output is unquoted:

```kdl
name criv
bin criv
version "0.10.1"
about "Local docs-to-code knowledge graph validator and query tool"
unknown_flags error
flag --usage hide=#true
cmd check help="Validate the vault against the source graph." {
    flag --format default=text {
        arg <FORMAT> {
            choices {
                choice text
                choice json
                choice github
            }
        }
    }
    flag --filter {
        arg <FILTER>
    }
    flag --fix
    flag --changed help="Validate safely scoped facts for the staged Git transaction." conflicts=--fix
}
cmd enforce help="Enforce the policy for one stage." {
    flag --stage required=#true {
        arg <STAGE> {
            choices {
                choice commit
                choice push
                choice ci
            }
        }
    }
    flag --pre-push help="Consume Git's pre-push ref-update records from standard input." hide=#true
    flag --remote-name hide=#true requires=--pre-push {
        arg <REMOTE_NAME>
    }
    flag --remote-url hide=#true requires=--pre-push {
        arg <REMOTE_URL>
    }
}
```

The existing unit test in `src/lib.rs` asserts `flag --usage hide=#true`. That
assertion still passes. Decision D4 from the audit is not needed.

Note also `unknown_flags error` in the spec, and `conflicts=--fix` and
`requires=--pre-push` written in the portable flag spelling.

## Correction 3: the spec endpoint needs `parse()`

Research [#188](https://github.com/TudorAndrei/criv/issues/188) says the derive
answers a `__usage_spec__` first word. The prototype refuses it:

```text
error: unrecognized subcommand '__usage_spec__'
```

The intercept sits in the process entry preamble, which reads
`std::env::args_os()` itself. `parse_from` takes words only and never sees it.
criv passes its own argument vector to `run(args)`, so criv must either call
`usage_argv::is_spec_request` itself, or move to `parse()`. The hidden `--usage`
flag works either way, so this changes nothing that users see.

## Correction 4: `parse_from` takes `&[&OsStr]`

Not `&[OsString]`. The caller must build the borrowed vector. criv's `run`
signature takes `Vec<String>`, so the migration adds two lines of conversion.

## Help output

Root, long form:

```text
criv 0.10.1
Local docs-to-code knowledge graph validator and query tool

Usage: criv <SUBCOMMAND>

Commands:
  check [FLAGS]
    Validate the vault against the source graph.

  enforce <--stage <STAGE>>
    Enforce the policy for one stage.

  help
    Print this message or the help of the given subcommand(s)

Flags:
  -h, --help     Print help
  -V, --version  Print version
```

`check`:

```text
Validate the vault against the source graph.

Usage: criv check [FLAGS]

Flags:
      --format <FORMAT>
    [possible values: text, json, github]
    (default: text)
      --filter <FILTER>
      --fix
      --changed          Validate safely scoped facts for the staged Git
                         transaction.
  -h, --help             Print help
```

`enforce`:

```text
Enforce the policy for one stage.

Usage: criv enforce <--stage <STAGE>>

Flags:
      --stage <STAGE>
    [possible values: commit, push, ci]
  -h, --help           Print help
```

Three points for the help contract ticket:

1. The required flag prints as `<--stage <STAGE>>` in the usage line. This is
   what `clean_required_flag_usage` corrects today.
2. The section word is `Flags:` and the placeholder is `[FLAGS]`. This is what
   `normalize_help_output` corrects today.
3. The hidden flags do not print. `remove_hidden_from_help` is dead code, as the
   research said.

## Error text and exit path

Every failure below leaves exit code 2, which is the criv contract.

```text
$ criv-proto check --bogus
error: unexpected argument '--bogus' found

$ criv-proto check --format toml
error: invalid value 'toml' for '--format'
  [possible values: text, json, github]

$ criv-proto check --fix --changed
error: the argument '--changed' cannot be used with '--fix'

$ criv-proto enforce --stage push --remote-name origin
error: the following required arguments were not provided:
  --pre-push

$ criv-proto check extra-word
error: unexpected argument 'extra-word' found
```

Each message keeps clap's wording. Each one adds a `Usage:` line and the words
`For more information, try '--help'.`

## The effect of `unknown_flags = "error"`

One declaration on the root gives the refusal at every level. `check --bogus`
fails, and `check extra-word` fails as well, because `check` declares no
positional argument. Without the setting, both words would bind as values.

## Correction 5: a positional prints in upper case

The audit says the five positionals print `<symbol>` and asks for an explicit
`value_name`. A `symbol: String` field in the prototype prints `<SYMBOL>`:

```text
Usage: criv query callers [--format <FORMAT>] <SYMBOL>

Arguments:
  <SYMBOL>  The symbol to look up.
```

This matches clap. Decision D2 is not needed, and the two test assertions in
`tests/cli_workflows.rs` do not break.

## Correction 6: a missing subcommand may make the `query` hint unnecessary

`criv query` with no subcommand prints the full help page and exits 2:

```text
$ criv-proto query
Query the knowledge graph.

Usage: criv query <SUBCOMMAND>

Commands:
  query callers [--format <FORMAT>] <SYMBOL>  List source symbols that call the requested symbol.
  query orphan-docs [--format <FORMAT>]  List documentation notes without citations.
  help  Print this message or the help of the given subcommand(s)

Flags:
  -h, --help  Print help
[exit 2]
```

criv today prints a clap error and adds a hand-written hint that lists the valid
`query` subcommands. The help page already lists them. The map holds a standing
decision that the hint stays. That decision now needs a second look.

A wrong subcommand still gives a short error:

```text
$ criv-proto query bogus
error: unrecognized subcommand 'bogus'
```

## What is still not shown

- The 14 real `query` subcommands and the six `flatten` sites.
- The `install-editor` and `adr` trees.
- Behavior under a real shell completion request.

# Usage Spec Export and Help Path Research

Date: 2026-08-22

## Question

How do the Usage spec export and the help path change when the `usage-rs`
derive owns the spec?

## Answer

The derive emits the KDL. `Cli::to_kdl()` gives the same document that
`criv --usage` prints today. `usage-cli` 6.0.0 still accepts that document on
stdin for `generate completion`, `generate markdown`, and `generate manpage`.

The hidden `--usage` flag keeps its exact output shape, but only if criv keeps
the flag. Criv must declare `--usage` itself and print `Cli::to_kdl()`. The
derive also adds a new `__usage_spec__` word, which is not in the spec document.

Only one of the four post-processing helpers dies:
`remove_hidden_from_help`. The renderer hides hidden entries natively. The
other three helpers must stay, or criv must accept a changed help page.

`usage-lib` stops being a direct dependency. `usage-rs` renders help from its
own static tables. Criv should keep `usage-lib` as a dev-dependency only, for
the round-trip test.

This research applies to `usage-rs` 6.0.0, `usage-lib` 6.0.0, and `usage-cli`
6.0.0. All three went to crates.io on 2026-08-22. The reviewed official source
is tag `v6.0.0`, commit
[`aa58dc48656c5fb52d8820a4c8e6a49f47981509`](https://github.com/jdx/usage/tree/aa58dc48656c5fb52d8820a4c8e6a49f47981509).

## 1. What emits the KDL

The derive writes four functions on the root type. See
[`derive/src/codegen.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/derive/src/codegen.rs#L1125-L1152):

```rust
pub fn command() -> &'static usage_argv::Command<'static>;
pub fn spec() -> &'static usage_argv::spec::Spec<'static>;
pub fn app() -> usage_argv::spec::SpecView<'static>;
pub fn to_kdl() -> ::std::string::String;
```

`to_kdl()` is the replacement for the current criv code. The method name is
`to_kdl`, not `spec_kdl`. See the official
[Spec output page](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/docs/rust/spec.md).

The derive also adds a spec endpoint. A command line whose first word is
`__usage_spec__` prints the KDL and exits `0`. The parser answers the word
before the parse, so the word is not in the parse tables and is not in the
printed document. See
[`argv/src/lib.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/lib.rs#L1080-L1095)
and
[`derive/src/codegen.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/derive/src/codegen.rs#L1716-L1740).
`#[usage(spec_endpoint = false)]` removes the endpoint.

The current criv code re-parses its own output:

```rust
let spec: usage::Spec = (&Cli::command()).into();
spec.to_string().parse().expect("derived usage spec should parse")
```

That round trip goes away. `to_kdl()` returns the finished document.

### The generators still accept the output

The emitted KDL parses back with `usage-lib`. A conformance test does this
check field by field. See
[`conformance/tests/spec_roundtrip.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/conformance/tests/spec_roundtrip.rs#L1-L14)
and its
[snapshot](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/conformance/tests/snapshots/spec_roundtrip__the_emitted_spec_is_stable.snap).

`usage generate completion`, `usage generate markdown`, and
`usage generate manpage` all read `--file -` from stdin in 6.0.0. See
[`cli/src/cli/generate/mod.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/cli/src/cli/generate/mod.rs#L56-L62),
[`completion.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/cli/src/cli/generate/completion.rs#L31),
[`markdown.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/cli/src/cli/generate/markdown.rs#L11),
and
[`manpage.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/cli/src/cli/generate/manpage.rs#L11).

Therefore the three README commands keep their shape.

### The consumer must be version 6

The 6.0.0 spec dialect changed. Two breaking changes touch criv:

- The derive lowers a flattened struct into a `flagset` node.
- A group node replaces a rule that no single flag can state.

See the
[6.0.0 release notes](https://github.com/jdx/usage/releases/tag/v6.0.0),
entries `(spec) breaking lower the derive's flatten into a flagset` and
`(spec) breaking a group, for the rule that no single flag can state`.

Criv uses `#[command(flatten)]` at six places in `src/query.rs`. Its spec will
therefore hold `flagset` and `use` nodes. `usage-lib` 6.0.0 reads those nodes
and expands them while it reads the file, so a parsed spec holds the expanded
flags. See
[`lib/src/spec/flagset.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/lib/src/spec/flagset.rs#L7-L30).

An older `usage-cli` cannot read those nodes. The official migration page states
the rule: pin the producer and the consumer to one revision. See
[Migrating from clap](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/docs/rust/migrating-from-clap.md).

## 2. The hidden `--usage` flag

The flag keeps the same output shape, but criv must keep the flag.

The `__usage_spec__` endpoint does not replace `--usage` in the document. The
official Spec output page states that the endpoint "is answered before the
parse — so it is not in your tables, cannot collide with a flag of yours, and
does not appear in the document it prints".

So criv must continue to declare the flag:

```rust
#[usage(long = "usage", hide)]
usage: bool,
```

The derive accepts `hide` on a field. See
[`derive/src/model.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/derive/src/model.rs#L5156).

The KDL writer quotes a flag name only when the flag has more than one
spelling. A single long flag stays unquoted. The canonical snapshot shows both
forms:

```kdl
flag --color help="colorize output" negate=--no-color default="true"
flag "-v --verbose" hide=#true count=#true
```

See the
[canonical KDL snapshot](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/conformance/tests/snapshots/spec_roundtrip__the_emitted_spec_is_stable.snap#L20-L21)
and the writer at
[`argv/src/spec.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/spec.rs#L2006).

`--usage` has one spelling. The emitted node is therefore
`flag --usage hide=#true`. The assertion in `src/lib.rs` stays correct.

`docs/query-reference.md` is generated from the same document. Its generation
does not change.

## 3. The four post-processing helpers

### `remove_hidden_from_help` — remove it

The renderer excludes hidden flags, hidden arguments, and hidden subcommands
from both the sections and the usage line. The source says so directly:

```rust
// Hidden entries are absent from the line as they are from the sections: help describes
// what a user is invited to type.
let flags: usize = meta.flags.iter().filter(|f| !f.hide).count();
```

See
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L499-L520)
for the usage line and
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L336-L365)
for the section filters. The official Help page confirms it: "`hide` removes an
entry from help, docs, and completions while still parsing".

The criv test `assert!(!root.contains("--usage"))` will pass without help.

### `normalize_help_usage` and `clean_required_flag_usage` — keep them

The native renderer writes the same angle brackets that criv strips today. A
required flag is angled like a required argument:

```rust
// A required flag is angled, like a required argument: the brackets are what
// say whether leaving it out is allowed.
let (open, close) = if flag_demanded(flag) { ('<', '>') } else { ('[', ']') };
let _ = write!(out, " {open}{}{close}", flag_usage(flag));
```

See
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L504-L514).

So `criv enforce` still renders `Usage: criv enforce <--stage <STAGE>>`. The
criv test `usage_help_cleans_required_flag_placeholders` expects
`Usage: criv enforce --stage <STAGE>`.

The synopsis override cannot fix this. `usage = "…"` applies only to the root
command. The renderer checks `path.len() <= 1` and states the rule in a
comment: "An explicit synopsis belongs to the program rather than every command
below it." See
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L543-L553).

Criv has three options:

1. Accept the angled form and change the test.
2. Make `--stage` not required, and give it a default or a group.
3. Keep a text replacement on the rendered page.

Option 3 works because criv can intercept the help request. See
[section 4](#4-the-help-path-and-the-usage-lib-dependency).

### `normalize_help_output` — keep it

The renderer writes `Flags:` as the default section heading, and `[FLAGS]` or
`<FLAGS>` in the usage line when a command has more than two flags. The inline
limit is 2. See
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L30),
[L375-L384](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L375-L384),
and
[L516-L518](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L516-L518).

`usage-lib` renders the same words. Its template says
`{{ group.heading | default(value="Flags") }}`. See
[`lib/src/docs/cli/templates/spec_template_long.tera`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/lib/src/docs/cli/templates/spec_template_long.tera#L102).
The two renderers agree, which the official Help page confirms.

Criv can remove one half of this helper natively. `help_heading = "Options"` on
every flag replaces the `Flags:` heading, because the renderer pushes the
default heading only when a flag has no declared heading. See
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L375-L384)
and the attribute at
[`derive/src/model.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/derive/src/model.rs#L5157).

That change costs 21 attributes and does not touch `[FLAGS]` in the usage line.
A three-line text replacement is simpler. Keep the helper.

### Summary

| Helper | After the migration |
| --- | --- |
| `remove_hidden_from_help` | Remove. The renderer hides hidden entries. |
| `normalize_help_usage` | Keep, or accept the changed synopsis. |
| `clean_required_flag_usage` | Keep, or accept the changed synopsis. |
| `normalize_help_output` | Keep. The renderer says `Flags` and `[FLAGS]`. |

## 4. The help path and the `usage-lib` dependency

`usage-lib` stops being a direct dependency. `usage-rs` covers help rendering.

The official Spec output page states it: "The derive compiles your declaration
into static tables that usage-argv parses and renders help from directly — no
KDL is parsed when your CLI runs, and usage-lib is not a dependency of your
binary."

The `usage-rs` manifest confirms it. Its dependencies are `usage-argv`,
`usage-derive`, `usage-test`, `usage-validation`, and `usage-config`. Its
default features are `spec`, `help`, and `diagnostics`. `usage-lib` appears
only under `[dev-dependencies]`. See
[`usage-rs/Cargo.toml`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/usage-rs/Cargo.toml).

Criv should therefore:

- Replace `usage = { package = "usage-lib", features = ["clap"] }` with
  `usage = { package = "usage-rs", version = "6.0" }` under `[dependencies]`.
- Add `usage-parser = { package = "usage-lib", version = "6.0" }` under
  `[dev-dependencies]`, for the round-trip test the official page recommends.

### The new render call

`usage::docs::cli::render_help(&spec, command, long)` becomes
`usage::help::render(Cli::spec(), cmd, long)`. It returns `Option<String>`, so
criv can still post-process the page. See
[`argv/src/help.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/help.rs#L2426-L2435).

### `help_request` goes away

Criv's `help_request` function maps argv to a command path and a `long` flag.
The parser does this work natively and returns the result as an error value:

- `-h` returns `Error::Help { cmd, long: false }`.
- `--help` returns `Error::Help { cmd, long: true }`.
- `help <command> <subcommand>` walks the whole path and returns
  `Error::Help { cmd, long: true }`.

See
[`argv/src/lib.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/lib.rs#L1930-L1961)
and
[`argv/src/lib.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/argv/src/lib.rs#L733).

So criv must use `Cli::parse_from`, not `Cli::parse()`, and must match on the
error. The official Help page gives the shape:

```rust
match Cli::parse_from(&argv) {
    Ok(cli) => run(cli),
    Err(Error::Help { cmd, long }) => {
        print!("{}", normalize(&usage::help::render(Cli::spec(), cmd, long).unwrap()));
    }
    Err(Error::Version { long }) => { /* … */ }
    Err(err) => { /* … */ }
}
```

`usage::Error` is `#[non_exhaustive]`. Always keep a fallback arm.

### One behavior change to decide

Criv prints the long help page to stdout with exit `0` when a user types `criv`
alone. `parse()` does not do that. `arg_required_else_help` prints the **short**
page to **stderr** and exits **2**. See
[`derive/src/codegen.rs`](https://github.com/jdx/usage/blob/aa58dc48656c5fb52d8820a4c8e6a49f47981509/derive/src/codegen.rs#L1535-L1546).

Criv should therefore keep `command: Option<Command>` and render the page
itself in the `None` arm. That holds the current exit code and stream, which
the map lists as a standing decision.

## Risk to the generated documentation

Three risks stand out.

First, the spec dialect changed in 6.0.0. `docs/query-reference.md` and any
completion, Markdown, or manpage output need `usage-cli` 6.0.0 or later. An
older installed `usage-cli` will fail to read the `flagset` nodes that criv's
six `#[command(flatten)]` sites produce. The README should name the required
version.

Second, the help page changes shape unless criv keeps two of the four helpers.
`tests/cli_workflows.rs` and the tests at the bottom of `src/lib.rs` are the
contract that will show this.

Third, `usage-rs` is experimental. Its compatibility policy allows changes to
the portable spec dialect in point releases. Criv should pin the version and
keep the round-trip test that parses `Cli::to_kdl()` with `usage-lib`.

## Conclusion

The derive owns the spec, and `Cli::to_kdl()` emits it. The generators still
accept the document, on the condition that criv and `usage-cli` share major
version 6. The hidden `--usage` flag survives with the same KDL shape, because
criv declares it. Only `remove_hidden_from_help` becomes unnecessary. `usage-lib`
leaves `[dependencies]` and belongs in `[dev-dependencies]` for the round-trip
test.

The help path stops being a spec transformation and becomes an error match.
That is the largest change in `src/lib.rs`.

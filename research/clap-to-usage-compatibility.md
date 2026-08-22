# criv clap Surface against the usage-rs Compatibility Matrix

Date: 2026-08-22

Research ticket: [#186](https://github.com/TudorAndrei/criv/issues/186).
Map: [#185](https://github.com/TudorAndrei/criv/issues/185).

## Question

Which criv clap declarations map cleanly to `usage-rs`, and which the audited
compatibility matrix marks **usage-only**, **lossy**, **partial**, **different**,
or **no**?

## Answer

Every criv clap declaration maps to `usage-rs` 6.0.0. The matrix marks no criv
declaration **no**, **lossy**, or **partial**. 43 of 48 audited items are clean,
mechanical rewrites.

Five items need a human decision. None of them blocks the migration:

1. The root `name = "criv"` must become `bin = "criv"`, not `name = "criv"`.
2. The five positional arguments lose their upper-case help placeholders.
3. Bare `criv query` prints a help page instead of one error line.
4. The exported KDL spec text changes, because a new writer emits it.
5. The `query` subcommand hint must find a new error variant, because
   `usage-argv` has no `InvalidSubcommand`.

The migration also removes work the map did not expect. `unknown_flags = "error"`
on the root reaches every subcommand, so criv does not set it 17 times. The
`help` subcommand, hidden-entry filtering, and nested help routing are built in,
so about 60 lines of hand-written help plumbing in `src/lib.rs` go away.

## Audited versions

| Item | Version | Evidence |
| ---- | ------- | -------- |
| criv | 0.11.0 | `Cargo.toml` |
| clap | 4.6.4 | `Cargo.lock` |
| clap_derive | 4.6.4 | `Cargo.lock` |
| usage-lib (current) | 3.5.6 | `Cargo.lock` |
| usage-rs | 6.0.0 | [crates.io](https://crates.io/api/v1/crates/usage-rs), published 2026-08-22 |
| usage-derive | 6.0.0 | [crates.io](https://crates.io/api/v1/crates/usage-derive) |
| usage-argv | 6.0.0 | [crates.io](https://crates.io/api/v1/crates/usage-argv) |
| clap_usage (bridge) | 5.0.0 | [crates.io](https://crates.io/api/v1/crates/clap_usage) |

The matrix is audited against clap 4.6.6 and clap_derive 4.6.4. criv locks clap
4.6.4. The difference is one patch release of `clap` and no release of
`clap_derive`.

Primary sources read for this audit:

- [`docs/rust/clap-compatibility.md`](https://github.com/jdx/usage/blob/main/docs/rust/clap-compatibility.md)
- [`docs/rust/migrating-from-clap.md`](https://github.com/jdx/usage/blob/main/docs/rust/migrating-from-clap.md)
- [`docs/rust/args-and-flags.md`](https://github.com/jdx/usage/blob/main/docs/rust/args-and-flags.md)
- The published sources of `usage-derive` 6.0.0 (`src/model.rs`, `src/codegen.rs`,
  `src/case.rs`) and `usage-argv` 6.0.0 (`src/lib.rs`, `src/help.rs`,
  `src/diagnostic.rs`, `src/spec.rs`).

## How to read the marks

The marks come from the matrix. This audit reports the **derive** column,
because criv removes clap completely and migrates from the Rust declaration.

The **bridge** column does not apply to criv. `clap_usage` is a separate crate,
it is still at 5.0.0, and criv will not call it after the migration. Every
matrix row that says **usage-only** in the bridge column, but **yes** in the
derive column, is a clean rewrite for criv.

## Root command: `src/lib.rs`

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[derive(Parser)] struct Cli` | `#[derive(usage::Cli)] struct Cli` | yes | None. |
| `#[command(name = "criv")]` | `#[usage(bin = "criv")]` | yes | **Decision D1.** usage splits clap's `name`. `bin` is the spelling in help and in the spec. `name` is only a friendly display name. `name = "criv"` alone does not set the binary name. |
| `#[command(version)]` | `#[usage(version)]` | yes | None. Both read `CARGO_PKG_VERSION` and add `--version` and `-V`. |
| `#[command(about = "Local docs-to-code…")]` | `#[usage(about = "…")]` | yes | None. |
| `#[arg(long = "usage", hide = true)] usage: bool` | `#[usage(long = "usage", hide)] usage: bool` | yes | The flag still parses. usage filters hidden entries out of help itself (`usage-argv/src/help.rs`), so `remove_hidden_from_help` is dead code. |
| `#[command(subcommand)] command: Option<Command>` | `#[usage(subcommand)] command: Option<Command>` | yes | None. `usage-derive` models an optional subcommand as `Kind::Subcommand { optional: true }`, so bare `criv` still gives `None` and criv still prints its own help. |
| (implicit) | `#[usage(unknown_flags = "error")]` | different | usage passes an unknown flag to the positionals by default. `"error"` restores clap's refusal. See "Unknown flags" below. |

## Subcommand enums

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[derive(Subcommand)] enum Command` (7 tuple variants) | `#[derive(usage::Subcommands)]` | yes | None. Tuple variants are covered. The compile error for tuple shapes applies to tuple `Cli` and `Args` **structs**, and criv has none. |
| `#[derive(Subcommand)] enum AdrCommand` (2 tuple variants) | `#[derive(usage::Subcommands)]` | yes | None. |
| `#[derive(Subcommand)] enum QueryCommand` (14 tuple variants) | `#[derive(usage::Subcommands)]` | yes | None. |
| `#[command(about = "Reconcile exact governed source renames…")]` on `ReconcileSources` | `#[usage(about = "…")]` | yes | None. |
| Doc comments on 16 variants | Doc comments | yes | None. The first paragraph is short help; the whole block is long help. Same rule as clap. |
| Variant name to command name | Default `to_kebab` | yes | criv keeps `install-editor`, `reconcile-sources`, `next-adr-id`, `attack-surface`, `cited-by`, `orphan-docs`. usage's default splits before every upper-case letter, and clap's default folds acronyms. No criv variant holds an acronym run, so both rules give the same words. |

## `ValueEnum` types

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `check::Format { Text, Json, Github }` | `#[derive(usage::ValueEnum)]` | yes | None. Words stay `text`, `json`, `github`. |
| `query::Format { Text, Json }` | `#[derive(usage::ValueEnum)]` | yes | None. |
| `query::CoverageBy { Module, Adr }` | `#[derive(usage::ValueEnum)]` | yes | None. |
| `query::NodeKind` (16 variants) | `#[derive(usage::ValueEnum)]` | yes | None. `MacroCallback` stays `macro-callback` under both naming rules. |
| `enforce::Stage { Commit, Push, Ci }` | `#[derive(usage::ValueEnum)]` | yes | None. |
| `install::editor::Editor { Code, Cursor }` | `#[derive(usage::ValueEnum)]` | yes | None. |
| `#[arg(value_enum)]` on a field | `#[usage(value_enum)]` | yes | None. `usage-derive` accepts the same word. |

## `Args` structs and their fields

All 16 `#[derive(clap::Args)]` structs become `#[derive(usage::Args)]`. The
matrix marks dedicated, reused, nested, and flattened `Args` types **yes** at
every layer.

### `init::InitOptions` (`src/init.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long)] no_skills: bool` | `#[usage(long)] no_skills: bool` | yes | None. Name stays `--no-skills`. |
| `#[arg(long)] force_skills: bool` | `#[usage(long)] force_skills: bool` | yes | None. |

### `install::InstallEditorOptions` (`src/install/editor.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long, value_enum)] editor: Editor` | `#[usage(long, value_enum)] editor: Editor` | yes | None. A bare `T` with no default stays required. |
| `#[arg(long)] dry_run: bool` | `#[usage(long)] dry_run: bool` | yes | None. |

A required flag prints as `<--editor <EDITOR>>` in the usage line
(`usage-argv/src/help.rs`). criv already trims that shape with
`clean_required_flag_usage`, so keep that helper or accept the new line.

### `adr::AdrOptions` (`src/adr.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[command(subcommand)] command: AdrCommand` | `#[usage(subcommand)] command: AdrCommand` | yes | The bare `T` makes the subcommand required, as in clap. Bare `criv adr` changes its output. See **Decision D3**. |

### `adr::ReconcileOptions` (`src/adr.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long)] base: String` | `#[usage(long)] base: String` | yes | None. Required, because the type has nowhere to put "absent". |
| `#[arg(long)] check: bool` | `#[usage(long)] check: bool` | yes | None. |

### `adr::source_reconcile::Options` (`src/adr/source_reconcile.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long, help = "Target branch or commit to compare with")] base: String` | `#[usage(long, help = "…")]` | yes | None. `usage-derive` accepts `help`. Prefer a doc comment, as the sibling struct already uses. |
| `#[arg(long, help = "Report a required reconciliation…")] check: bool` | `#[usage(long, help = "…")]` | yes | None. |

### `check::CheckOptions` (`src/check.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long, value_enum, default_value_t = Format::Text)] format: Format` | `#[usage(long, value_enum, default_value_t = Format::Text)]` | yes | None. `usage-derive` accepts `default_value_t` and calls `ToString` on the expression. `default = "text"` is the portable spelling. A default makes the field optional in the grammar, exactly as clap does. |
| `#[arg(long)] filter: Option<String>` | `#[usage(long)] filter: Option<String>` | yes | None. |
| `#[arg(long)] fix: bool` | `#[usage(long)] fix: bool` | yes | None. |
| `#[arg(long, conflicts_with = "fix")] changed: bool` | `#[usage(long, conflicts_with = "fix")]` | yes | None. `usage-derive` keeps the `conflicts_with` word and resolves a bare clap argument id as a fallback after the portable `--fix` spelling. The portable spelling is `conflicts("--fix")`. A selector that names nothing is a **compile error**, not a silent no-op. |

### `query::QueryOptions` (`src/query.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[command(subcommand)] command: QueryCommand` | `#[usage(subcommand)] command: QueryCommand` | yes | Bare `criv query` changes its output. See **Decision D3**. |

### `query::OutputOptions` (`src/query.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long, value_enum, default_value_t = Format::Text)] format: Format` | `#[usage(long, value_enum, default_value_t = Format::Text)]` | yes | None. Same as `check::CheckOptions::format`. |

### The six flattened query structs

`SymbolOptions`, `NoteOptions`, `DecisionOptions`, `CoverageOptions`,
`NodesOptions`, and `DiffOptions` each hold
`#[command(flatten)] output: OutputOptions`.

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[command(flatten)] output: OutputOptions` (6 sites) | `#[usage(flatten)] output: OutputOptions` | yes | None. Parsing and help topology compose. |
| Relationships across a flatten boundary | — | lossy | **Not used by criv.** Both criv relationships (`conflicts_with` in `check.rs`, `requires` in `enforce.rs`) live inside one struct. The lossy row never applies. |
| `symbol: String` (positional) | `symbol: String` | yes | **Decision D2.** The placeholder becomes `<symbol>`, not `<SYMBOL>`. |
| `note_id: String` (positional) | `note_id: String` | yes | Placeholder becomes `<note-id>`. See D2. |
| `adr_id: String` (positional) | `adr_id: String` | yes | Placeholder becomes `<adr-id>`. See D2. |
| `ref_a: String`, `ref_b: String` (positional) | same | yes | Placeholders become `<ref-a>` and `<ref-b>`. See D2. |
| `#[arg(long, value_enum)] by: Option<CoverageBy>` | `#[usage(long, value_enum)]` | yes | None. |
| `#[arg(long, value_enum)] kind: Option<NodeKind>` | `#[usage(long, value_enum)]` | yes | None. |
| `#[arg(long)] without_docs: bool` | `#[usage(long)] without_docs: bool` | yes | None. |

### `watch::WatchOptions` (`src/watch.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long)] once: bool` | `#[usage(long)] once: bool` | yes | None. |

### `enforce::EnforceOptions` (`src/enforce.rs`)

| clap declaration | usage-rs spelling | Mark | Behavior difference |
| ---------------- | ----------------- | ---- | ------------------- |
| `#[arg(long, value_enum)] stage: Stage` | `#[usage(long, value_enum)] stage: Stage` | yes | None. Required, and printed as `<--stage <STAGE>>` in the usage line. |
| `#[arg(long, hide = true)] pre_push: bool` | `#[usage(long, hide)] pre_push: bool` | yes | None. A hidden flag still parses. |
| `#[arg(long, hide = true, requires = "pre_push")] remote_name: Option<String>` | `#[usage(long, hide, requires = "pre_push")]` | yes | None in criv's use. The matrix marks the bridge column **usage-only**, because clap has a setter and no getter. criv does not use the bridge, so the mark does not apply. The portable spelling is `requires("--pre-push")`. |
| `#[arg(long, hide = true, requires = "pre_push")] remote_url: Option<String>` | same | yes | Same as above. |

## Clap features criv does not use

The audit found no criv use of these. Every one is a matrix row that would
otherwise need a decision.

| clap surface | Matrix mark | criv |
| ------------ | ----------- | ---- |
| `from_global` | no | Not used. |
| Builder code, `ArgMatches`, `FromArgMatches` | non-goal | See **Decision D4**: `src/lib.rs` calls `CommandFactory` three times. |
| `env` on a field | lossy (bridge only) | Not used. |
| `Vec<T>`, `Option<Vec<T>>`, `num_args`, `value_delimiter` | yes | Not used. criv has no repeatable or variadic field. |
| `short` on a field | yes | Not used. Only the built-in `-h` and `-V`. |
| `value_parser` callbacks | usage-only | Not used. |
| `default_missing_value`, `default_value_if` | usage-only (bridge) | Not used. |
| `ArgGroup`, `exclusive`, `overrides_with` | yes | Not used. |
| `global` flags | yes | Not used. |
| `allow_external_subcommands`, `multicall` | yes | Not used. |
| `infer_subcommands`, `infer_long_args` | no | Not used. clap has both off by default, so usage's refusal changes nothing. |
| `help_template`, `term_width` | different / no bridge | Not used. |
| `help_heading`, `display_order`, `next_line_help` | yes | Not used. |
| Elvish completion | no | Not used. criv ships no completions today. |

## Unknown flags

The matrix marks unknown flags **different** at every layer. usage treats an
unknown flag-like word as a value; clap refuses it.

The map's standing decision says "every command sets `unknown_flags = "error"`".
The published source shows this is not needed. `usage-argv` 6.0.0 documents the
field as inherited:

> The parser carries the effective value down as it descends, so a command that
> states nothing costs nothing.
> — `usage-argv-6.0.0/src/lib.rs`, on `CommandMeta::unknown_flags`

The descent copies the mode only when a child states one
(`usage-argv-6.0.0/src/lib.rs`), and the spec writer emits the property only
where it differs from the inherited value (`usage-argv-6.0.0/src/spec.rs`).

`unknown_flags` is also **not** in the root-only list in
`Cli::check_position` (`usage-derive-6.0.0/src/model.rs`), so a command may still
override it.

Therefore: one `#[usage(unknown_flags = "error")]` on the root `Cli` covers all
17 commands. The exported KDL carries it once, at the top.

## Error text: most assertions survive

`usage-argv` 6.0.0 reproduces clap's diagnostic wording. Checked against
`usage-argv-6.0.0/src/diagnostic.rs` and its tests.

| criv test assertion (`tests/cli_workflows.rs`) | usage 6.0.0 output | Survives |
| ---------------------------------------------- | ------------------ | -------- |
| `unrecognized subcommand 'bogus'` | `error: unrecognized subcommand 'bogus'` | Yes |
| `unexpected argument '--kind'` | `error: unexpected argument '--kind' found` | Yes, as a substring |
| `unexpected argument 'extra'` | `error: unexpected argument 'extra' found` | Yes |
| `unexpected argument` (init) | same | Yes |
| `invalid value 'invalid'` | `error: invalid value 'invalid' for '--by <BY>'` | Yes |
| `module, adr` | `[possible values: module, adr]` | Yes |
| `code, doc, decision` | `[possible values: code, doc, …]` | Yes |
| `required arguments` | `error: the following required arguments were not provided:` | Yes |
| `<SYMBOL>` | `<symbol>` | **No.** See D2. |
| `<REF_B>` | `<ref-b>` | **No.** See D2. |
| `Valid query subcommands:` | criv appends this itself | **Needs D5.** |

Only two wording assertions break, and one criv-owned hint needs a new trigger.
The map listed "the size of the change inside `tests/cli_workflows.rs`" as not
yet specified. The answer is: small. Decision D2 can reduce it to zero.

## Items that need a human decision

### D1 — `name` is not `bin`

clap's `#[command(name = "criv")]` sets the binary name. usage splits the idea.
`bin` is the spelling used in help and in the emitted spec; `name` is a friendly
display name (`docs/rust/args-and-flags.md`, container attributes).

criv's unit test asserts the spec contains `bin criv`. Write
`#[usage(bin = "criv")]`. Do not translate `name` to `name`.

**Recommendation:** use `bin = "criv"`. This is a decision only because the
mechanical rename gives the wrong result silently.

### D2 — Positional help placeholders lose their capitals

clap shouts a positional's name: field `symbol` prints `<SYMBOL>`. usage prints
the declared name as written. `usage-derive` shouts a **flag's value**
placeholder (`shout` in `src/model.rs`), but a positional keeps its kebab name,
and `usage-argv::help::arg_usage` writes it between angle brackets unchanged.

This affects five positionals: `symbol`, `note_id`, `adr_id`, `ref_a`, `ref_b`.
Two `tests/cli_workflows.rs` assertions read `<SYMBOL>` and `<REF_B>`.

Two options:

- Declare `#[usage(arg, value_name = "SYMBOL")]` on each of the five fields.
  Help, errors, and the spec keep today's text. No test changes.
- Accept `<symbol>` and update the two assertions. Less code, changed output.

**Recommendation:** declare `value_name`. criv is a git-hook tool; its help text
is read by people who already know the current shape.

### D3 — A missing subcommand prints a help page

`usage-argv` handles `Error::MissingSubcommand` by rendering the command's short
help page, and says why:

> clap prints the command's help page here, including the available subcommands,
> while keeping exit 2; an error plus only `<SUBCOMMAND>` tells the reader what
> is missing and withholds the list they need to fix it.
> — `usage-argv-6.0.0/src/diagnostic.rs`

This changes `criv query` and `criv adr` with no subcommand: a help page instead
of one error line. The exit code stays 2, so git hooks and CI are unaffected.
The root `criv` is unaffected, because `Option<Command>` still gives `None`.

No test asserts the current wording. The change is an improvement.

**Recommendation:** accept it, and record it in the ADR as a changed output.

### D4 — The clap help plumbing in `src/lib.rs` goes away

`src/lib.rs` builds its own help path on top of `clap::CommandFactory`:
`usage_spec`, `usage_help`, `help_request`, `command_for_path`,
`remove_hidden_from_help`, `normalize_help_usage`, `clean_required_flag_usage`,
and `normalize_help_output`.

usage 6.0.0 supplies most of it:

- The `help` subcommand is built in, and it resolves a nested path such as
  `criv help query coverage` without descending into it
  (`usage-argv-6.0.0/src/lib.rs`).
- `-h` gives short help and `--help` gives long help, as clap has them.
- Hidden flags, arguments, and subcommands are filtered out of help, of the
  usage line, and of completions (`usage-argv-6.0.0/src/help.rs`). Delete
  `remove_hidden_from_help`.
- `usage_argv::help::render(spec, cmd, long)` replaces
  `usage::docs::cli::render_help`. The signature is the same shape.

Two normalizations remain a choice:

- usage 6 writes the section heading `Flags:` and the placeholders `[FLAGS]` and
  `<FLAGS>`; clap writes `Options:` and `[OPTIONS]`. criv rewrites them today in
  `normalize_help_output`.
- A required flag still prints as `<--stage <STAGE>>`, which
  `clean_required_flag_usage` trims today.

**Recommendation:** keep both normalizations for one release, and remove the
rest. Decide separately whether criv adopts usage's `Flags:` vocabulary.

### D5 — The `query` subcommand hint needs a new trigger

`parse_error` in `src/lib.rs` appends "Valid query subcommands: …" when clap
reports `ErrorKind::InvalidSubcommand` for a `query` argument list. The map keeps
this hint.

`usage_argv::Error` has no `InvalidSubcommand` variant. The audited variant list
is `UnknownFlag`, `MissingFlagValue`, `UnexpectedArg`, `ArgRequiresDoubleDash`,
`SubcommandConflict`, `TooDeep`, `MissingRequired`, `DuplicateFlag`,
`InvalidChoice`, `VarTooFew`, `VarTooMany`, `ConflictingFlags`, `InvalidValue`,
`MissingGroup`, `MissingSubcommand`, `Help`, `MissingArgsHelp`, `HelpAll`, and
`Version`.

The diagnostic layer still **prints** "unrecognized subcommand 'bogus'". It
chooses that wording for `Error::UnexpectedArg` when the command has
subcommands. So criv must key the hint on `Error::UnexpectedArg` plus the
`query` path, not on a dedicated variant.

**Recommendation:** match `Error::UnexpectedArg` and keep the existing `query`
path guard. Note that usage already prints a "did you mean" tip, so the hint may
be redundant. Confirm the hint is still wanted.

## Other facts the specification needs

### The exported KDL text changes

`criv --usage` prints the spec. Today usage-lib 3.5.6 writes it. After the
migration `Cli::to_kdl()` writes it through `usage-argv-6.0.0/src/spec.rs`, which
quotes flag forms: `flag "--usage" hide=#true`, and `bin "criv"`.

The unit test in `src/lib.rs` asserts the unquoted `flag --usage hide=#true` and
`bin criv`. Both assertions must change. The integration test at
`tests/cli_workflows.rs` only checks an absence, so it is safe.

This is a public artifact. Record the change in the ADR and in the changelog.

### Parse entry point

criv calls `Cli::try_parse_from(std::iter::once("criv").chain(...))`. usage
supplies `try_parse_from` with the same clap-shaped contract, and also
`parse_from` for words without argv0. `parse_from` removes the `once("criv")`
prefix. Both are generated by the derive.

### Repeated flags

usage is permissive and last-one-wins by default; `args_override_self = false`
opts into strict duplicate checking. criv has no test that gives a scalar flag
twice, so no contract is at risk. Leave the default.

### Dependency graph

The migration removes 8 third-party crates from the compiled binary and leaves
none, per the measured table in `docs/rust/migrating-from-clap.md`. `usage-rs`
needs only `proc-macro2`, `quote`, `syn`, and `unicode-ident`, which are
build-time only.

## Counts

| Item | Count |
| ---- | ----- |
| `Args` structs | 16 |
| Subcommand enums | 3 |
| `ValueEnum` types | 6 |
| Long flags | 22 (21, plus the root `--usage`) |
| Positional arguments | 5 |
| `Option<T>` fields | 5 |
| `Vec<T>` fields | 0 |
| `hide = true` | 4 |
| `default_value_t` | 2 |
| `conflicts_with` | 1 |
| `requires` | 2 |
| Audited items total | 48 |
| Clean mechanical rewrites | 43 |
| Items needing a decision | 5 |
| Items marked **no**, **lossy**, or **partial** | 0 |

## Conclusion

The compatibility matrix does not block this migration. criv uses a small,
conservative part of clap, and `usage-rs` 6.0.0 covers all of it in the derive
layer.

Do these five things in the implementation specification:

1. Write `bin = "criv"` on the root, not `name`.
2. Give the five positionals an explicit `value_name`.
3. Set `unknown_flags = "error"` once, on the root.
4. Delete `remove_hidden_from_help`, `help_request`, and `command_for_path`;
   keep `normalize_help_output` and `clean_required_flag_usage` for now.
5. Re-key the `query` subcommand hint on `Error::UnexpectedArg`.

Then update two assertions in `tests/cli_workflows.rs` and two in the `src/lib.rs`
unit tests. Everything else in the CLI behavior contract holds unchanged.

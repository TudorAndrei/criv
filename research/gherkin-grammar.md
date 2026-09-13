# Gherkin Grammar and ast-grep Language Contract

Date: 2026-09-13

Ticket: [#202](https://github.com/TudorAndrei/criv/issues/202).
Map: [#198](https://github.com/TudorAndrei/criv/issues/198).

## Question

criv extracts Source symbols with tree-sitter and evaluates ADR policy
patterns with ast-grep. Gherkin support needs the exact contract of both.

## Answer

ast-grep registers no Gherkin language, and no usable tree-sitter Gherkin
grammar exists. Use the `gherkin` crate (MIT OR Apache-2.0, 0.16.0,
2026-04-23) for extraction. It costs 99,568 bytes of release binary. It gives
Rust structs, not tree-sitter nodes, so it keeps no comments, it does not
separate Scenario Outline from Scenario, and it returns nothing at all for a
file with one bad keyword. Localization changes only a `keyword` string; it
changes no type name.

An ADR `policy.patterns` rule therefore cannot reach a `.feature` file.
Behaviour policy needs another mechanism.

---

## Which Gherkin parser criv should use

### There is no Gherkin in ast-grep, so a tree-sitter grammar buys nothing

The `ast-grep-language` dependency and criv's own `tree-sitter-*`
dependencies are separate lists. A tree-sitter grammar is only worth the cost
when it fills both lists, as
[ADR-0119](https://github.com/TudorAndrei/criv/blob/main/docs/adr/0119-first-class-elixir-language-support.md)
did for Elixir. Gherkin can fill neither list, for the reasons below.

### No `tree-sitter-gherkin` package is published anywhere

- crates.io returns no result for `tree-sitter-gherkin`. Search:
  `https://crates.io/api/v1/crates?q=tree-sitter-gherkin`.
- npm returns `{"error":"Not found"}` for `tree-sitter-gherkin`. Request:
  `https://registry.npmjs.org/tree-sitter-gherkin`.
- The `tree-sitter` GitHub organization holds no `gherkin` repository.
  Request: `gh api orgs/tree-sitter/repos`.
- The `tree-sitter-grammars` GitHub organization holds no `gherkin`
  repository. Request: `gh api orgs/tree-sitter-grammars/repos`.
- `nvim-treesitter` has no `gherkin` entry. Request:
  `gh api search/code -f q="gherkin repo:nvim-treesitter/nvim-treesitter"`
  returns `total_count: 0`.

Only unpublished personal repositories exist. A GitHub search for
`tree-sitter-gherkin` returns nine, the largest with 6 stars.

### The grammar that Helix uses cannot parse ordinary Gherkin

Helix pins
[`SamyAB/tree-sitter-gherkin`](https://github.com/SamyAB/tree-sitter-gherkin)
at revision `43873ee8de16476635b48d52c46f5b6407cb5c09`. See
[`languages.toml`](https://github.com/helix-editor/helix/blob/master/languages.toml),
lines 4630-4639. The repository is MIT, 6 stars, last pushed 2024-07-04.

Its
[`grammar.js`](https://github.com/SamyAB/tree-sitter-gherkin/blob/main/grammar.js)
rejects valid Gherkin:

- `title: $ => /[A-Z][a-zA-Z ]+/`. A title must start with a capital letter
  and hold only letters and spaces. `Pay with <method>` fails. `place an
  order` fails. Any digit or punctuation fails.
- `tag: $ => token(seq('@', /[a-z_]+/))`. A tag must be lowercase.
  `@Smoke` fails. `@wip-2` fails.
- `feature: seq(optional($.tag), ...)`. One tag only. `@billing @web` fails.
- `steps: seq(optional($.given_steps), $.when_step, $.then_steps)`. A
  scenario must hold at least one `When` step and at least one `Then` step,
  in that order. A `Given`-only scenario fails.
- `scenario_keyword` is `choice('Scenario', 'Example')` with `:`. There is no
  `Scenario Outline` rule and no `Examples` rule at all.
- Every keyword is a literal English string. There is no `# language:` rule.

Its
[`Cargo.toml`](https://github.com/SamyAB/tree-sitter-gherkin/blob/main/Cargo.toml)
also states `version = "0.0.1"`, `tree-sitter = "~0.20.10"`, and
`repository = "https://github.com/tree-sitter/tree-sitter-gherkin"`, which
does not exist. criv uses `tree-sitter = "0.26.3"`, so the generated parser
would need regeneration.

### The most complete grammar is a one-day project

[`DevAbdullahUk/tree-sitter-gherkin`](https://github.com/DevAbdullahUk/tree-sitter-gherkin)
(MIT) does cover `Scenario Outline`, `Examples`, doc strings, data tables,
multiple tags, and comments. Its rules are `source_file`, `comment`,
`tag_list`, `tag`, `feature`, `rule_header`, `background`, `scenario`,
`examples`, `step`, `step_keyword`, `step_text`, `quoted_string`,
`parameter`, `text_fragment`, `name_line`, `description`, `narrative_line`,
`description_line`, `data_table`, `table_row`, `table_cell`, `doc_string`.
It has no `# language:` rule.

`gh api repos/DevAbdullahUk/tree-sitter-gherkin` reports
`created_at: 2026-07-14T20:03:30Z` and `pushed_at: 2026-07-14T20:29:50Z`,
0 stars, and 0 forks. It was created and abandoned in 27 minutes. It is not
a maintenance story that criv can depend on.

### The official cucumber parser has no Rust implementation

[`cucumber/gherkin`](https://github.com/cucumber/gherkin) is MIT, 403 stars,
and active (`pushed_at: 2026-09-13`). Its top level holds `c`, `cpp`,
`dart`, `dotnet`, `elixir`, `go`, `java`, `javascript`, `perl`, `php`,
`python`, and `ruby`. There is no `rust` directory. Request:
`gh api repos/cucumber/gherkin/contents`.

The parser is generated from
[`gherkin.berp`](https://github.com/cucumber/gherkin/blob/main/gherkin.berp)
by the Berp tool. Generating a Rust target is new work, not a dependency.

### Recommendation: the `gherkin` crate

[`gherkin` 0.16.0](https://crates.io/crates/gherkin/0.16.0), repository
[`cucumber-rs/gherkin`](https://github.com/cucumber-rs/gherkin).

| Fact | Value |
| --- | --- |
| Licence | `MIT OR Apache-2.0` |
| Latest release | 0.16.0, 2026-04-23 |
| Previous releases | 0.15.0 (2025-12-12), 0.14.0 (2023-07-14) |
| Downloads | 16,932,413 |
| MSRV | 1.88 |
| Runtime dependencies | `peg 0.6.3`, `textwrap 0.16`, `thiserror 2.0`, `typed-builder 0.23` |
| Transitive normal crates | 53 |

Facts from `https://crates.io/api/v1/crates/gherkin`,
`https://crates.io/api/v1/crates/gherkin/0.16.0/dependencies`, and the
crate's own
[`Cargo.toml.orig`](https://github.com/cucumber-rs/gherkin/blob/v0.16.0/Cargo.toml).

Maintenance state: active but slow. Three releases in three years, with a
1,144-day gap between 0.14.0 and 0.15.0. The
[changelog](https://github.com/cucumber-rs/gherkin/blob/v0.16.0/CHANGELOG.md)
shows real upstream alignment work in 0.15.0 and 0.16.0. It is the parser
that the `cucumber` crate uses, which explains the download count. It is not
the official cucumber parser; it is an independent pure-Rust reimplementation
by the `cucumber-rs` organization.

One stale point: it pins `peg = "0.6.3"`, released 2019. The current `peg`
is 0.8.6 (2026-05-04). Source:
`https://crates.io/api/v1/crates/peg`.

### Size cost against ADR-0015

Measured, not estimated. Two binaries were built in one crate with exactly
the release profile from
[ADR-0015](https://github.com/TudorAndrei/criv/blob/main/docs/adr/0015-size-optimized-release-profile.md)
(`strip = true`, `opt-level = "z"`, `lto = true`, `codegen-units = 1`,
`panic = "abort"`) on `aarch64-apple-darwin`, Rust edition 2024:

| Binary | Bytes |
| --- | --- |
| Baseline: read a file, print its length | 286,128 |
| Same, plus `gherkin::Feature::parse` and a full AST walk | 385,696 |
| **Delta** | **99,568** |

99,568 bytes is 1.01% of the 9,815,840-byte `criv` binary that ADR-0015
records. This is small, and ADR-0015 asks that future size work "start with
dependency and feature analysis". Two notes for that analysis:

- `textwrap 0.16.3` pulls `icu_segmenter 2.3.0`, `icu_collections`,
  `icu_locale_core`, `icu_provider`, `zerovec`, `yoke`, and `potential_utf`.
  Most of the 53 transitive crates come from this one path. `gherkin` exposes
  no feature to drop `textwrap`.
- `gherkin` has a default `parser` feature that adds `typed-builder`.
  Turning it off removes `Feature::parse` as well, so criv must keep it.

## Node shapes

The `gherkin` crate has no node type. It has Rust structs, so criv gets a
typed tree, not a syntax tree. Field lists are from
[`src/lib.rs`](https://github.com/cucumber-rs/gherkin/blob/v0.16.0/src/lib.rs)
at version 0.16.0.

```text
Feature    keyword: String
           name: String
           description: Option<String>
           background: Option<Background>
           scenarios: Vec<Scenario>
           rules: Vec<Rule>
           tags: Vec<String>
           span: Span
           position: LineCol
           path: Option<PathBuf>

Rule       keyword, name, description, background: Option<Background>,
           scenarios: Vec<Scenario>, tags, span, position

Background keyword, name, description, steps: Vec<Step>, span, position

Scenario   keyword, name, description, steps: Vec<Step>,
           examples: Vec<Examples>, tags, span, position

Examples   keyword, name: Option<String>, description,
           table: Option<Table>, tags, span, position

Step       keyword: String, ty: StepType, value: String,
           docstring: Option<String>, table: Option<Table>, span, position

Table      rows: Vec<Vec<String>>, span, position

StepType   Given | When | Then

Span       start: usize, end: usize
LineCol    line: usize, col: usize
```

Observed output from the reference feature file below, produced by a binary
that links `gherkin` 0.16.0:

```gherkin
# a leading comment
@billing @web
Feature: Checkout
  As a shopper I want to pay.

  Background: A signed-in shopper
    Given a signed-in shopper

  Rule: One order per basket
    Background: A full basket
      Given a basket with 2 items

    @smoke
    Scenario: Place an order
      Given a basket
      # an inline comment
      When I place the order
      """
      a doc string
      """
      Then the order exists
        | name  | value |
        | a     | 1     |

    Scenario Outline: Pay with <method>
      Given a basket
      When I pay with <method>
      Then the payment is <result>

      @slow
      Examples: Cards
        | method | result   |
        | visa   | accepted |
        | amex   | declined |
```

```text
feature  keyword="Feature" name="Checkout" tags=["billing","web"]
         description=Some("As a shopper I want to pay.") background=true
         scenarios=0 rules=1 span=Span{start:34,end:730}
         position=LineCol{line:3,col:1}
  rule   keyword="Rule" name="One order per basket" tags=[]
         background=true scenarios=2 span=Span{start:150,end:730}
         position=LineCol{line:9,col:3}
    scenario keyword="Scenario" name="Place an order" tags=["smoke"]
             steps=3 examples=0 position=LineCol{line:14,col:5}
      step keyword="Given " ty=Given value="a basket"
      step keyword="When " ty=When value="I place the order"
           docstring=Some("\na doc string\n")
      step keyword="Then " ty=Then value="the order exists"
           table=Some([["name","value"],["a","1"]])
    scenario keyword="Scenario Outline" name="Pay with <method>" tags=[]
             steps=3 examples=1 position=LineCol{line:25,col:5}
      step keyword="Given " ty=Given value="a basket"
      step keyword="When " ty=When value="I pay with <method>"
      step keyword="Then " ty=Then value="the payment is <result>"
      examples keyword="Examples" name=Some("Cards") tags=["slow"]
               table=Some([["method","result"],["visa","accepted"],
                           ["amex","declined"]])
```

Seven facts that the design must absorb:

1. **Scenario Outline is not a distinct type.** Both branches of the parser
   `rule scenario()` build a `Scenario`. Only `keyword` differs, and
   `examples` is non-empty. See
   [`src/parser.rs`](https://github.com/cucumber-rs/gherkin/blob/v0.16.0/src/parser.rs),
   the two alternatives of `rule scenario()`. There is no `ScenarioType`
   enum in 0.16.0.
2. **Comments are discarded.** Every comment rule in the PEG grammar is
   wrapped in `quiet!{}` and the result is dropped:
   `rule comment() = quiet!{comment_no_nl() nl_eof()}`. No struct has a
   comment field. A criv `feature:` symbol can carry no doc comment.
3. **Tags lose the `@`.** `rule tag() = "@" s:tag_char()+ { s.join("") }`
   returns `["billing", "web"]`, not `["@billing", "@web"]`.
4. **Step keywords keep a trailing space.** `keyword="Given "`. Block
   keywords do not: `keyword="Scenario"`, `keyword="Scenario Outline"`.
5. **`span` excludes leading tags.** `span.start` is captured after the tag
   list, so `span` for the reference Feature starts at byte 34, after
   `# a leading comment` and `@billing @web`. `position` is the line and
   column of the keyword, 1-based.
6. **`StepType` normalizes `And`, `But`, and `*`.** Only `Given`, `When`,
   and `Then` exist. The literal keyword survives in `Step::keyword`.
   Version 0.16.0 also treats an `And`-like or `But`-like keyword at the
   start of a scenario as a `Given` step
   ([#53](https://github.com/cucumber-rs/gherkin/pull/53)).
7. **Doc string content keeps its leading and trailing newline.**
   `docstring=Some("\na doc string\n")`. `rule docstring()` accepts both
   `"""` and ` ``` ` delimiters and runs `textwrap::dedent` on the body.
   This is the only use of `textwrap` in the crate.

## Error recovery on an incomplete file

There is none. `Feature::parse` returns
`Result<Feature, ParseError>`, and `Feature::parse_path` returns
`Result<Feature, ParseFileError>`. A failure gives no partial `Feature`, so
criv would get zero symbols from the file. Measured cases:

| Input | Result |
| --- | --- |
| Truncated last step (`When I`) | Parses. The partial step becomes `value="I"`. |
| Scenario with no name (`Scenario:`) | Parses. `name=""`. |
| One typed keyword (`Scenaro:`) | `Error at 8:20: {"unknown keyword"}`. No symbols. |
| No `Feature:` header | `Error at 1:14: {"unknown keyword"}`. No symbols. |
| Empty file | `Error at 2:1: {...}`. No symbols. |

`ParseError` exposes only a `Display` impl,
`"Error at {line}:{col}: {expected:?}"`. Its `position` and `expected`
fields are private, so criv cannot read the location without parsing the
message string. `GherkinEnv` also holds a `last_error` and a `fatal_error`
for `EnvError::UnsupportedLanguage`, `EnvError::UnknownKeyword`, and
`EnvError::InconsistentCellCount`, but those are crate-private
(`pub(crate) last_error`).

This contradicts the Elixir precedent. ADR-0119 requires that criv "use the
partial Tree-sitter result for an Elixir file with parse errors" and that
"safe declarations before and after the error remain available". A PEG
parser cannot do this. A typed keyword in one scenario silently removes
every scenario in the file from the graph.

The truncated case is worse than a clean failure: the file parses, and the
half-written step enters the graph with wrong text.

## Whether ast-grep registers a Gherkin language

**No.** This is the decisive finding.

`ast-grep-language` exposes a closed enum, not a registry. All 28 variants
of `SupportLang` in version 0.44.1, which criv pins, are:

```text
Bash, C, Cpp, CSharp, Css, Dart, Go, Elixir, Haskell, Hcl, Html, Java,
JavaScript, Json, Kotlin, Lua, Markdown, Nix, Php, Python, Ruby, Rust,
Scala, Solidity, Swift, Tsx, TypeScript, Yaml
```

Source: `SupportLang` in `ast-grep-language-0.44.1/src/lib.rs`, and
`SupportLang::all_langs()` in the same file. The published crate has one
module per language and there is no `gherkin.rs`. A `grep -n feature` over
that file returns nothing, so no extension table maps `.feature`. The
official
[supported-languages reference](https://ast-grep.github.io/reference/languages.html)
names no Gherkin, no Cucumber, and no `.feature` extension for the current
release either. (Its table also omits Dart, which the enum does hold, so
the enum is the authority.)

### What this means for criv today

criv's `src/structural.rs` types the language as `SupportLang`:

```rust
pub struct CompiledPolicy {
    language: SupportLang,
    matcher: CompiledMatcher,
}
```

Two consequences follow, both already in the code:

1. An ADR that declares `language: gherkin` fails to compile its policy.
   `parse_language` calls `SupportLang::from_str` and reports
   ``unsupported ast-grep language `gherkin` ``.
2. Even with a valid language, `find_policies_batch` skips a `.feature` file
   with no diagnostic:

   ```rust
   for source_file in vault.source_files() {
       let Some(language) = SupportLang::from_path(source_file) else {
           continue;
       };
   ```

   `SupportLang::from_path` maps by extension and knows no `feature`, so the
   file is dropped silently.

### The dynamic-language loading path

ast-grep does have one, and it does not fit criv.

`ast-grep-dynamic` (MIT, 0.45.3, 2026-08-31,
`https://crates.io/crates/ast-grep-dynamic`) provides `DynamicLang`,
`CustomLang`, and `DynamicLang::register(Vec<Registration>)`. A
`Registration` holds `lang_name`, `lib_path`, `symbol`, `meta_var_char`,
`expando_char`, and `extensions`. Registration calls `libloading::Library`
and `dlopen`s a compiled tree-sitter dynamic library. The comment on
`register` states it "should be called exactly once before use", so it is a
process-global one-shot.

The user-facing form is `customLanguages` in `sgconfig.yml`
([documentation](https://ast-grep.github.io/advanced/custom-language.html),
[`sgconfig` reference](https://ast-grep.github.io/reference/sgconfig.html)):

```yaml
customLanguages:
  mojo:
    libraryPath: mojo.so
    extensions: [mojo]
    outlineRules: outline/mojo.yml
    expandoChar: _
```

Three blockers:

- It needs a compiled `.so`, `.dylib`, or `.dll` per target triple. criv
  ships one static binary per platform under
  [ADR-0014](https://github.com/TudorAndrei/criv/blob/main/docs/adr/0014-tag-triggered-release-binary-workflow.md).
  Shipping a sidecar library is a release-workflow change, not a dependency
  change.
- `DynamicLang` is a different type from `SupportLang`. `CompiledPolicy`
  would need a language enum of its own.
- It still needs a tree-sitter Gherkin grammar to compile, and none is
  usable.

### The third path, for completeness

`ast_grep_core::Language` is a public trait
(`ast-grep-core-0.44.1/src/language.rs`) with `kind_to_id`, `field_to_id`,
and `build_pattern`. criv could implement it for a statically linked
Gherkin grammar and avoid both `SupportLang` and the dynamic loader. This
is the only path that keeps one static binary, but it too needs a usable
tree-sitter Gherkin grammar, so it is blocked by the same missing grammar.

### Conclusion for behaviour policy

An ADR `policy.patterns` rule cannot reach a feature file. Behaviour policy
needs another mechanism. criv's own `Language` enum
(`src/source/graph.rs`, line 332) is independent of `SupportLang`, so the
Source-graph half of
[#199](https://github.com/TudorAndrei/criv/issues/199) is unaffected: adding
a `Gherkin` variant beside `Rust`, `TypeScript`, `JavaScript`, `Python`,
`Go`, and `Elixir` needs no ast-grep change.

## Whether keyword localization changes the node names

**No for the `gherkin` crate. Yes in effect for any tree-sitter grammar.**

The `gherkin` crate loads the official keyword table. Its
[`src/languages.json`](https://github.com/cucumber-rs/gherkin/blob/v0.16.0/src/languages.json)
is a copy of
[`gherkin-languages.json`](https://github.com/cucumber/gherkin/blob/main/gherkin-languages.json)
from the official repository, and `build.rs` generates a keyword table per
language code with `heck`, `quote`, and `syn`. `GherkinEnv::new(language)`
selects the table, and a `# language: xx` first line switches it at parse
time (`rule language_directive()` in `src/parser.rs`).

Measured, with the same structure as the reference file above and a
`# language: fr` header:

```text
feature  keyword="Fonctionnalité" name="Paiement" tags=["facturation"]
  rule   keyword="Règle" name="Une commande par panier"
    scenario keyword="Scénario" name="Passer une commande" tags=["rapide"]
      step keyword="Soit " ty=Given value="un panier"
      step keyword="Quand " ty=When value="je passe la commande"
      step keyword="Alors " ty=Then value="la commande existe"
    scenario keyword="Plan du Scénario" name="Payer avec <methode>"
      step keyword="Soit " ty=Given value="un panier"
      examples keyword="Exemples" name=Some("Cartes")
```

The struct names are unchanged. `StepType` still resolves to `Given`,
`When`, and `Then`. Only the `keyword: String` field carries localized
text. The design rule follows directly:

> Derive a criv symbol kind from the Rust type, never from `Step::keyword`
> or `Scenario::keyword`. A kind derived from the keyword string would be
> localized, and `scenario:` would become `scénario:` in a French vault.

Two limits remain:

- `Background` is `Contexte` in French and is reached through the same
  `background` field, so it needs no keyword read.
- `Scenario` and `Scenario Outline` can only be told apart by the keyword
  string or by `examples.is_empty()`. The keyword string is localized, so
  `examples.is_empty()` is the only localization-safe test, and it is wrong
  for a `Scenario Outline` that has no `Examples` block yet.

For a tree-sitter grammar the answer is different. Every grammar reviewed
hardcodes English keyword literals (`'Feature'`, `'Given '`) and none has a
`# language:` rule, so a localized file does not parse at all. A grammar
that did support localization would have to either add one node kind per
language or use a scanner, and node names are the whole ast-grep pattern
surface.

## Reproduction

Every measurement above comes from one throwaway crate:

```toml
[package]
name = "sizetest"
version = "0.1.0"
edition = "2024"

[dependencies]
gherkin = "0.16.0"

[[bin]]
name = "sizetest"
path = "src/main.rs"

[[bin]]
name = "baseline"
path = "src/baseline.rs"

[profile.release]
strip = true
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
```

`sizetest` calls `gherkin::Feature::parse` and prints every field.
`baseline` reads the file and prints its length. Build with
`cargo build --release` and compare with `stat -f %z`.

## Unconfirmed

- Whether any of the seven remaining personal `tree-sitter-gherkin`
  repositories is better than the two reviewed. Only
  `SamyAB/tree-sitter-gherkin` (the Helix pin) and
  `DevAbdullahUk/tree-sitter-gherkin` (the most complete) were read.
- The binary-size delta on Linux and Windows targets. Only
  `aarch64-apple-darwin` was measured.
- Whether the `icu_*` crates that `textwrap` pulls in are already in criv's
  dependency graph. If they are, the real delta is smaller than 99,568
  bytes. This needs a measurement inside criv, not in a throwaway crate.
- Whether `gherkin` 0.16.0 handles a data table with an escaped `\|` cell
  the same way the official parser does. The changelog claims 0.15.0 fixed
  escape sequences ([#47](https://github.com/cucumber-rs/gherkin/issues/47)),
  but this was not tested against the official `testdata` corpus in
  [`cucumber/gherkin/testdata`](https://github.com/cucumber/gherkin/tree/main/testdata).
- Whether `gherkin` accepts the Markdown-with-Gherkin format that
  [`MARKDOWN_WITH_GHERKIN.md`](https://github.com/cucumber/gherkin/blob/main/MARKDOWN_WITH_GHERKIN.md)
  defines. This is out of scope for #198 under ADR-0032, so it was not
  tested.

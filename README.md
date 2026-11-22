# criv

`criv` is a local ADR system of record and policy enforcement tool. It keeps
architectural decisions connected to the code they govern, with
validated source references, structural policies, queries, and local State that
other tools can read.

Architecture rendering and runtime Gherkin integration belong in the separate
[Concord toolkit](https://github.com/TudorAndrei/concord). criv has no LikeC4
runtime dependency. See [the extraction guide](docs/likec4-integration.md) for
the breaking changes.

## What

`criv` checks the configured ADR folder and enforces its source policies:

- `criv init` creates the vault files, ADR directory, and agent skills.
- `criv check` validates ADRs, source references, and ADR rules.
- `criv watch --once` writes the local state used by tools and editors.
- `criv query` and `criv enforce` inspect and protect the graph.

`.criv/` is local generated state and should stay ignored by git.

Before you upgrade to the operating-system watch lock, stop all older
`criv watch` processes. The `.criv/watch.lock` file is persistent diagnostic data.
Do not delete this file while `criv` runs.

## Why

Most repository documentation drifts because it is written beside the code but
not checked against it. `criv` makes that relationship explicit:

- Docs can point at source files, symbols, structural patterns, and notes.
- ADRs can govern the code paths and policy patterns they affect.
- Validation catches broken wiki-links, unresolved source references, malformed
  metadata, and unsafe ADR changes before they land.
- Generated state gives editors, automation, and review tools a stable local
  snapshot of the documentation graph.

The goal is not to replace source control, linters, or an editor. The goal is to
make design knowledge inspectable and enforceable in the same local workflow as
the code.

## Quick start

Add criv to the target repository's `mise.toml`:

```toml
[tools."github:TudorAndrei/criv"]
version = "latest"
```

Then initialize the repository:

```sh
mise install
criv init
criv watch --once
criv check
```

Tell your agent to read the current documentation and, when available, past
project conversations before it creates ADRs. It must create ADRs only for
lasting decisions supported by that evidence.

For local development in this repository:

```sh
mise install
```

The mise postinstall hook runs `hk install --mise`, so Git hooks execute through
`mise x` and use the tool versions from `mise.toml`. The hook policy is
documented in [docs/tooling.md](docs/tooling.md).

User-facing features and flows for criv itself are declared in executable
Gherkin files under `tests/features/`; ADRs record architectural decisions.
Run `cargo nextest run --test acceptance` for the Cucumber suite, or
`cargo nextest run --workspace` to include it with all Rust tests. See
[the testing guide](docs/testing.md) for authoring and migration conventions.

You can also run the CLI from a checkout without installing it globally:

```sh
cargo run -- check
cargo run -- query coverage
```

## Common workflow

Use the quick start above for a new repository. Use these commands when the
documentation or code changes:

```sh
criv watch --once
criv check
```

### Initialization, checks, and command reference

Initialize a repository:

```sh
criv init
```

This creates the default vault layout:

```text
criv.toml
docs/
  adr/  # The document folder criv reads
.criv/
.agents/skills/
```

Only the configured ADR folder enters criv checks, queries, and local state.
Other documentation can use lat.md or another tool and needs no criv metadata.
ADRs retain their structure, history, source references, and policy checks.
Links from ADRs to external Markdown documents check file existence only;
external headings and contents belong to the other documentation tool.
Existing `criv.toml` settings also control the folder created by `criv init`.
See [ADR-20261115-limit-document-processing-to-the-decision-folder](docs/adr/20261115-limit-document-processing-to-the-decision-folder.md).

`criv init` does not create Git hooks and does not touch `core.hooksPath`.
Setting that config replaces the hook directory wholesale, which would silently
disable whichever hook runner the repository already uses. Wire criv into your
own runner instead — see
[docs/tooling.md](docs/tooling.md) for hk and lefthook configuration, per
[ADR-0054](docs/adr/0054-criv-does-not-install-git-hooks.md).

`criv init` does not create or update editor files. The optional viewer is
local-only and is not published to an extension registry. Each criv release
archive includes the viewer. Install it explicitly into one selected editor:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Use `--dry-run` to validate the editor and bundled viewer without changing
editor state. This separation is recorded in
[ADR-0087](docs/adr/0087-keep-editor-setup-out-of-init.md).

Use `--no-skills` if you do not want the generated agent skills:

```sh
criv init --no-skills
```

`criv check` may report that installed agent skills are out of date. Refresh
only those criv-owned generated files with `criv init --force-skills`; it does
not create or change hooks, vault configuration, or `.gitignore`. Refreshing
deliberately replaces any local edits to those skill files. JSON and GitHub
check output never include this advisory note.

Check the vault before committing documentation or code changes:

```sh
criv check
criv check --changed
criv check --format json
criv check --format github
criv check --filter broken-link
```

`--changed` checks the staged Git transaction. It checks document structure,
validates local facts authored by changed vault files, and scopes policy scans
to changed sources. Changes that can alter global identity or resolution—such
as renames, deletions, ADR edits, or configuration edits—automatically run the
full check. A passing changed check is a pre-commit fast-path result, not a
full-vault validity claim; plain `criv check` remains the CI and manual
authority. All checks are read-only. Formatting is separate repository tooling;
`check --fix` has been removed, per
[ADR-0148](docs/adr/0148-separate-markdown-formatting-from-vault-validation.md).

criv requires non-empty frontmatter `id`, `kind`, and `title`, plus a supported
ADR `status`. Notes require exactly one H1 matching their title, ignoring case
and punctuation. Active ADRs require the H2 sections `Context`, `Decision`, and
`Consequences`, once each and in order. Accepted historical records are left
unchanged when an accepted successor takes over. Heading links remain checked.

General Markdown style is not a criv concern. This repository uses oxfmt through
its independent formatting hooks and `npm run format:markdown`. Consumer vaults
can choose another formatter or none; criv never starts one.

`--format` selects the diagnostic output. `text` is the default, `json` emits one
object per diagnostic for editors and scripts, and `github` emits workflow
annotation commands so diagnostics appear inline on a pull request.

Every diagnostic carries the command or edit that repairs it, printed as a `fix:`
line in text and as a `fix` field in JSON. A failed run ends with one `next:`
command. Failures with a defined recovery also carry a bracketed code, such as
`[not-a-vault]`, so a script or an agent can key on the code rather than the
prose.

criv refuses to run outside a vault. `criv check`, `criv query`, `criv watch`,
`criv enforce`, and `criv adr` require a `criv.toml` in the working directory,
and report `[not-a-vault]` with `fix: criv init` when there is none. Only
`criv init` and `criv install-editor` run without one.

`--filter` keeps only diagnostics whose text contains the given substring. It
narrows the exit status as well as the output: `criv check --filter broken-link`
exits zero when the only errors in the vault are of some other kind. Use it for
focused local inspection, not as a gate.

Local snapshots retain the latest 20 distinct state publications by default.
Configure another positive bound with `[state] keep`. criv applies retention
automatically after successful State publication. Use `criv query diff` to
compare `latest`, a retained hash, or a Git reference. See
[the local state reference](docs/state-reference.md) for the configuration and
query contract.

ADRs can enforce structural policy directly from their frontmatter:

```yaml
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Prefer structured diagnostics.
```

`criv check` and `criv enforce` parse those inline ast-grep rules from the ADR
each time they run. They are also the only persistent named patterns: a policy
named `no-println` in `ADR-0005` is addressed as `ADR-0005/no-println` for links
and state.

Elixir policies use `language: elixir` or its `language: ex` alias. Both names
scan `.ex` and `.exs` files with the Elixir parser.

criv also gives Elixir modules, protocols, implementations, functions, macros,
guards, callbacks, directives, and calls first-class Source graph identities.
It uses callable arity in canonical selectors. See
[the Elixir source support guide](docs/elixir-support.md) for selector forms,
short aliases, ambiguity, static call limits, and the no-Mix boundary.

Use a filtered check to inspect policy diagnostics for one decision:

```sh
criv check --filter ADR-0005
```

Refresh generated state when docs or source files change:

```sh
criv watch --once
```

Ask the graph focused questions:

```sh
criv query coverage
criv query nodes --kind code --without-docs
criv query governs ADR-0001
criv query governing src/main.rs
criv query diff latest latest
```

Run `criv query --help` for every `criv query` subcommand, positional argument,
and flag. The CLI is the reference, so an agent working in your repository has
the same information you do.

Every query takes `--format text`, `--format json`, or `--format ndjson`, and
`--limit <N>` to bound the answer before it is printed. `criv --usage-json`
prints the whole command tree, with every flag, choice, and default.

Run the same enforcement path used by hooks and CI:

```sh
criv enforce --stage commit
criv enforce --stage push
criv enforce --stage ci
criv enforce --stage ci --format json
```

`--format json` reports the stage, the counts, the comparison basis, the
violations, and the failure code with its repair, instead of prose.
`criv watch --once --format json` does the same for a refresh.

`criv.toml` configures which stages run, and the import policies that
`criv enforce` gates on. See [docs/configuration.md](docs/configuration.md).

## Creating a new ADR

A new ADR takes its id from its UTC creation date and its title. Create it
with:

```sh
criv adr new "Cache compiled policies"
```

This writes the draft `docs/adr/20261117-cache-compiled-policies.md` with the
id `ADR-20261117-cache-compiled-policies`, the title, `status: draft`, and
empty Context, Decision, and Consequences sections, and prints its path. Fill
in the sections, add `governs:`, and set the status. The slug is the title in
lowercase, with its words joined by `-`; it must use only ASCII letters and
digits. Add `--date YYYYMMDD` to use another date. The command fails, and
writes nothing, when an ADR with that id already exists.

The path makes the id unique, so two branches need no coordination. They clash
only when they add an ADR with the same date and title, and Git then reports an
add/add conflict at the merge. criv does not allocate, reserve, or renumber ids.

ADRs created before this rule keep their `ADR-NNNN` ids and `NNNN-slug.md`
names. `criv enforce` rejects an added ADR file with a legacy `NNNN-` name and
reports the `legacy-adr-id` code; rename the file to the one that
`criv adr new <title>` creates, and update its links. See
[ADR-20261117-date-and-slug-decision-identifiers](docs/adr/20261117-date-and-slug-decision-identifiers.md).

## Reconciling ADR source governance

An exact source rename does not change an ADR's meaning, but its `governs:`
path still needs to follow the file. After committing the source rename, inspect
the repair against the integration target:

```sh
criv adr reconcile-sources --base origin/main --check
```

If a repair is required, run the command without `--check`. It accepts only
one-to-one Git-proven renames, rewrites exact `governs:` path scalars, validates
the vault, writes an ignored `.criv/source-reconcile.json` transaction receipt,
and creates a dedicated
`docs(adr): reconcile renamed source scopes` commit. It does not infer directory
moves, rewrite broad globs, or update other source-reference forms.

Deletion is not a rename and cannot be reconciled mechanically. Add a new
accepted ADR that explains the removal and lists the former decision in
`supersedes:`. Until that successor is accepted, `criv check` and State refresh
fail for the unresolved active scope. An accepted successor makes the former
ADR historical and deactivates its policies; criv never writes that decision
for you.

Generate a Usage spec for completions, Markdown, or manpages:

```sh
criv --usage | usage generate completion --file - zsh criv
criv --usage | usage generate markdown --file - --out-file docs/cli.md
criv --usage | usage generate manpage --file - --out-file criv.1
```

The `usage` CLI that reads the spec must be the same major version as the
`usage-rs` that criv builds with. The spec dialect changes between major
versions, so an older reader cannot parse a newer document.

The installed pre-commit and pre-push hooks run their validation phases
automatically. The common manual fixer is:

```sh
mise run fix
```

The hosted core profile is defined in `hk.pkl`, not as a mise task. For targeted
diagnosis, run one step with `hk check --all --step <name>`; for example,
`hk check --all --step hawk`. `hk check --all --plan` lists every hosted core
step. Companion checks use the package commands documented in
[[tooling|Tooling and Git Hooks]].

## Obsidian Extension

The repository contains a maintained Obsidian companion under
`.obsidian/plugins/criv/`. `criv init` does not copy it into another repository.
The plugin is a UI over `.criv/state.json`: it reads the CLI-generated graph
state, validates the schema version, renders source and pattern context, offers
source autocomplete, shows lazy thumbnails for verified documentation images,
and opens verified images and PDFs with Obsidian. It delegates shared State
validation and projection logic to the `criv-wasm` crate.

Build the plugin when working on its templates or WASM helper:

```sh
npm ci
npm --prefix .obsidian/plugins/criv run build
```

Then run `criv watch --once` from the repository root so Obsidian has fresh
state to read.

The plugin intentionally stays a consumer of local state. Source editing,
validation, policy enforcement, and graph generation remain owned by the CLI.

## VS Code-Compatible Extension

The companion extension under `extensions/vscode-criv/` targets VS Code and
VS Code-derived editors such as Cursor. It reads `.criv/state.json`, shows a
native state tree and status summary, opens AST-aware source selectors, surfaces
`criv check --format json` diagnostics, and opens verified documentation images
and PDFs with the native editor.

For local development:

```sh
npm ci
npm --prefix extensions/vscode-criv run build
npm --prefix extensions/vscode-criv run test
npm --prefix extensions/vscode-criv run test:integration
```

## Releases

Release steps are documented in [docs/releasing.md](docs/releasing.md).

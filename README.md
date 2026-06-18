# criv

`criv` is a local CLI for keeping repository documentation connected to the code
it describes. It turns a repo's `docs/` folder into a validated knowledge graph
of notes, source references, architectural decisions, policy patterns, and local
state that other tools can read.

## What

`criv` gives a codebase an executable documentation layer:

- `criv init` creates the vault files, ADR directory, Git hooks, agent runtime
  skills, and Obsidian companion plugin scaffold.
- `criv check` validates Markdown formatting, note schema, ADR placement,
  wiki-links, source targets, Mermaid C4 diagram references, pattern
  references, and ADR supersession rules.
- `criv watch --once` writes `.criv/state.json` and content-addressed local
  snapshots for downstream tools.
- `criv query ...` asks the graph about targets, citations, ADR governance,
  coverage, callers, callees, attack surface, C4 elements and relationships,
  focused C4 Code projections, diffs, and orphaned docs.
- `criv search ...` searches files, text, notes, and structural policy patterns.
- `criv enforce --stage commit|push|ci` runs stage-aware documentation and policy
  checks.

`.criv/` is local generated state and should stay ignored by git.

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

## Install

Install the Rust CLI from the repository:

```sh
cargo install --git https://github.com/TudorAndrei/criv criv
```

For local development in this repository, install the pinned project tools:

```sh
mise install
```

The mise postinstall hook runs `hk install --mise`, so Git hooks execute through
`mise x` and use the tool versions from `mise.toml`. The hook policy is
documented in [docs/tooling.md](docs/tooling.md).

You can also run the CLI from a checkout without installing it globally:

```sh
cargo run -- check
cargo run -- query coverage
cargo run -- search --files main
```

## How

Initialize a repository:

```sh
criv init
```

This creates the default vault layout:

```text
criv.toml
docs/
  SKILL.md
  skills/
  adr/
.criv/
.githooks/
.obsidian/plugins/criv/
```

When run inside a non-bare Git repository, `criv init` also creates
`.githooks/pre-commit` and `.githooks/pre-push`, then sets the local Git config
`core.hooksPath` to `.githooks`. The generated hooks run `criv watch --once`,
`criv check`, and stage-specific `criv enforce` commands during normal Git
workflows. They use `criv` from `PATH`, falling back to `./target/debug/criv`
in development checkouts when that binary exists.

Use `--no-skills`, `--no-obsidian`, or `--no-hooks` if you do not want those
generated templates or hooks:

```sh
criv init --no-skills
criv init --no-obsidian
criv init --no-hooks
```

Existing hook files and non-criv `core.hooksPath` settings are preserved by
default. Use `--force-hooks` to replace `.githooks/pre-commit`,
`.githooks/pre-push`, and an existing `core.hooksPath` value.

Check the vault before committing documentation or code changes:

```sh
criv check
criv check --fix
```

Refresh generated state when docs or source files change:

```sh
criv watch --once
```

Ask the graph focused questions:

```sh
criv query next-adr-id
criv query coverage
criv query nodes --kind code --without-docs
criv query governs ADR-0001
criv query governing src/main.rs
criv query c4-elements ADR-0026
criv query c4-relationships ADR-0026
criv query c4-code src/c4.rs
criv query diff latest latest
```

`c4-code` emits a Mermaid `classDiagram` from the source graph for a focused
file or component/module glob. Use it for C4 Code-level inspection of a narrow
implementation area, not as a whole-application architecture diagram.

Search code and notes:

```sh
criv search --files main
criv search --grep "watch --once"
criv search --notes "Obsidian"
criv search --rule docs/policies/no-unsafe.yml
```

Run the same enforcement path used by hooks and CI:

```sh
criv enforce --stage commit
criv enforce --stage push
criv enforce --stage ci
```

Generate a Usage spec for completions, Markdown, or manpages:

```sh
criv --usage | usage generate completion --file - zsh criv
criv --usage | usage generate markdown --file - --out-file docs/cli.md
criv --usage | usage generate manpage --file - --out-file criv.1
```

In this repository, the common manual tasks are:

```sh
mise run pre-commit
mise run pre-push
mise run check
mise run fix
```

## Obsidian Extension

`criv init` installs an Obsidian companion plugin scaffold under
`.obsidian/plugins/criv/` unless `--no-obsidian` is passed. The plugin is a UI
over `.criv/state.json`: it reads the CLI-generated graph state, validates the
schema version, renders source and pattern context, offers source autocomplete,
and delegates shared helper logic to the `criv-wasm` crate.

Build the plugin when working on its templates or WASM helper:

```sh
npm --prefix .obsidian/plugins/criv ci
mise run plugin-build
```

Then run `criv watch --once` from the repository root so Obsidian has fresh
state to read.

The plugin intentionally stays a consumer of local state. Source editing,
validation, policy enforcement, and graph generation remain owned by the CLI.

## Releases

Release steps are documented in [docs/releasing.md](docs/releasing.md).

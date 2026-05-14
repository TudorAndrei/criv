# criv

`criv` is a local CLI for keeping repository documentation connected to the code
it describes. It turns a repo's `docs/` folder into a validated knowledge graph
of notes, source references, architectural decisions, policy patterns, and local
state that other tools can read.

## What

`criv` gives a codebase an executable documentation layer:

- `criv init` creates the vault files, ADR directory, agent runtime skills, and
  Obsidian companion plugin scaffold.
- `criv check` validates Markdown formatting, note schema, ADR placement,
  wiki-links, source targets, pattern references, and ADR supersession rules.
- `criv watch --once` writes `.criv/state.json` and content-addressed local
  snapshots for downstream tools.
- `criv query ...` asks the graph about targets, citations, ADR governance,
  coverage, callers, callees, attack surface, diffs, and orphaned docs.
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
.obsidian/plugins/criv/
```

Use `--no-skills` or `--no-obsidian` if you do not want those generated
templates:

```sh
criv init --no-skills
criv init --no-obsidian
```

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
criv query diff latest latest
```

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
cd .obsidian/plugins/criv
npm install
npm run build
```

Then run `criv watch --once` from the repository root so Obsidian has fresh
state to read.

The plugin intentionally stays a consumer of local state. Source editing,
validation, policy enforcement, and graph generation remain owned by the CLI.

## Releases

Release steps are documented in [docs/releasing.md](docs/releasing.md).

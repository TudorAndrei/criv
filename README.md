# criv

`criv` turns a repository's `docs/` folder into a validated map of documentation, code references, and architectural decisions.

This repository currently contains a local Rust CLI and vault model. It implements:

- `criv init` for idempotent vault scaffolding
- `criv check` for Markdown formatting, note schema, ADR placement, wiki-link, target, pattern-reference, and supersession validation
- `criv query next-adr-id`
- `criv query targets <note-id>`
- `criv query cites <note-id>`
- `criv query cited-by <note-id>`
- `criv query governs <ADR-ID>`
- `criv query governing <symbol>`
- `criv query coverage`
- `criv query nodes [--kind code|doc] [--without-docs]`
- `criv query callers <symbol>`
- `criv query callees <symbol>`
- `criv query attack-surface`
- `criv query diff <snapshot-a> <snapshot-b>`
- `criv query orphan-docs`
- `criv search --files <query>`
- `criv search --grep <text>`
- `criv search --notes <text>`
- lexical structural-search fallbacks for `criv search '<pattern>'`, `--pattern-id`, and `--rule`
- `criv watch --once` state and local snapshot writing
- `criv enforce --stage commit|push|ci`

The tree-sitter, ast-grep, fff-search, fastembed, native lint integrations, and full Obsidian rendering integrations are intentionally isolated as the next implementation layer.

## Try It

```sh
cargo run -- check
cargo run -- check --fix
cargo run -- query next-adr-id
cargo run -- search --files main
```

`criv check` embeds `rumdl` as a Rust crate for Markdown formatting checks. Use
`criv check --fix` to apply fixable Markdown formatting changes before
validation.

## Git Hooks

This repository includes an [hk](https://hk.jdx.dev/) configuration in `hk.pkl`
and a [mise](https://mise.jdx.dev/) integration in `mise.toml`. Install the
project tools with:

```sh
mise install
```

The mise postinstall hook runs `hk install --mise`, so Git hooks execute through
`mise x` and use the tool versions from `mise.toml`. The config sets
`HK_PKL_BACKEND=pklr`, so hk does not require a separate `pkl` CLI.

Useful manual commands:

```sh
mise run commit-msg -- .git/COMMIT_EDITMSG
mise run pre-commit
mise run pre-push
mise run check
mise run fix
```

## Vault Layout

```text
criv.toml
docs/
  SKILL.md
  skills/
  adr/
.criv/
```

`.criv/` is local state and is ignored by git.

Release steps are documented in [docs/releasing.md](docs/releasing.md).

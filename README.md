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
- `criv state list|prune` inspects and bounds those ignored local snapshots.
- `criv query ...` asks the graph about targets, citations (`cites`,
  `cited-by`), ADR governance (`governs`, `governing`), coverage, callers,
  callees, attack surface, C4 elements and relationships, focused C4 Code
  projections, diffs, and orphaned docs.
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

To pin criv in another repository with mise, add the following to that
repository's `mise.toml`:

```toml
[tools."github:TudorAndrei/criv"]
version = "0.7.0"

[tools."github:TudorAndrei/criv".platforms]
macos-arm64 = { asset_pattern = "criv-aarch64-apple-darwin.tar.gz" }
linux-arm64 = { asset_pattern = "criv-aarch64-unknown-linux-gnu.tar.gz" }
linux-x64 = { asset_pattern = "criv-x86_64-unknown-linux-gnu.tar.gz" }
windows-x64 = { asset_pattern = "criv-x86_64-pc-windows-msvc.zip" }
```

Then install the pinned tool and initialize the repository:

```sh
mise install
criv init
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
.vscode/extensions.json
.obsidian/plugins/criv/
```

`criv init` does not create Git hooks and does not touch `core.hooksPath`.
Setting that config replaces the hook directory wholesale, which would silently
disable whichever hook runner the repository already uses. Wire criv into your
own runner instead — see
[docs/tooling.md](docs/tooling.md) for hk and lefthook configuration, per
[ADR-0054](docs/adr/0054-criv-does-not-install-git-hooks.md).

`criv init` recommends the VS Code-compatible companion extension through
`.vscode/extensions.json` using the stable extension ID `criv.vscode-criv`. It
does not install the extension into VS Code, Cursor, or any other editor by
default. A future explicit install path can shell out to a selected editor CLI
such as `code` or `cursor` with a published extension ID or local `.vsix`.

Use `--no-skills`, `--no-obsidian`, or `--no-vscode` if you do not want those
generated templates or editor recommendations:

```sh
criv init --no-skills
criv init --no-obsidian
criv init --no-vscode
```

`criv check` may report that installed agent skills are out of date. Refresh
only those criv-owned generated files with `criv init --force-skills`; it does
not create or change hooks, editor scaffolding, vault configuration, or
`.gitignore`. Refreshing deliberately replaces any local edits to those skill
files. JSON and GitHub check output never include this advisory note.

Check the vault before committing documentation or code changes:

```sh
criv check
criv check --changed
criv check --fix
criv check --format json
criv check --format github
criv check --filter broken-link
```

`--changed` checks the staged Git transaction. It lints changed Markdown,
validates local facts authored by changed vault files, and scopes policy scans
to changed sources. Changes that can alter global identity or resolution—such
as renames, deletions, ADR edits, or configuration edits—automatically run the
full check. A passing changed check is a pre-commit fast-path result, not a
full-vault validity claim; plain `criv check` remains the CI and manual
authority. `--changed` is read-only and cannot be combined with `--fix`, per
[ADR-0067](docs/adr/0067-staged-changes-are-a-partial-check-scope.md).

`--fix` applies the Markdown formatting fixes rumdl can make automatically. It
rewrites any Markdown file `criv check` lints, anywhere inside the repository
root, so it is not limited to the vault docs directory. Which files are linted
is controlled by the `include` and `exclude` lists in your rumdl configuration;
excluding a directory there is how you keep `--fix` away from it. Writes are
confined to the repository root and never follow symlinks, per
[ADR-0044](docs/adr/0044-vault-write-confinement.md).

`--format` selects the diagnostic output. `text` is the default, `json` emits one
object per diagnostic for editors and scripts, and `github` emits workflow
annotation commands so diagnostics appear inline on a pull request.

`--filter` keeps only diagnostics whose text contains the given substring. It
narrows the exit status as well as the output: `criv check --filter broken-link`
exits zero when the only errors in the vault are of some other kind. Use it for
focused local inspection, not as a gate.

Local snapshots retain the latest 20 distinct state publications by default.
Configure another positive bound with `[state] keep`, inspect snapshots with
`criv state list`, and preview cleanup with `criv state prune --dry-run`. See
[the local state reference](docs/state-reference.md) for the command and
configuration contract.

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

`criv check`, `criv search --rule ADR-NNNN`, and `criv enforce` parse those
inline ast-grep rules from the ADR each time they run. They are also the only
persistent named patterns: a policy named `no-println` in `ADR-0005` is
addressed as `ADR-0005/no-println` for links, state, and a focused search.

Use `criv search --pattern-id ADR-NNNN/local-id` to inspect one persistent
policy. With no `--paths`, the search uses the owning ADR's effective
`governs` scope; pass `--paths` to deliberately override it. For unnamed
exploratory searches, keep the pattern on the command line and specify its
language:

```sh
criv search --pattern-id ADR-0005/no-println
criv search --rule ADR-0005
criv search --lang rust 'println!($$$ARGS)'
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

See [docs/query-reference.md](docs/query-reference.md) for every `criv query`
subcommand, positional argument, flag, and example.

`c4-code` emits a Mermaid `classDiagram` from the source graph for a focused
file or component/module glob. Use it for C4 Code-level inspection of a narrow
implementation area, not as a whole-application architecture diagram.

Search code and notes:

```sh
criv search --files main
criv search --grep "watch --once"
criv search --notes "Obsidian"
criv search --pattern-id ADR-0005/no-println
criv search --rule ADR-0005
```

### Semantic note search (optional)

`criv search --notes <query> --semantic` matches notes by meaning rather than by
substring. It is off by default and requires two independent gates:

```sh
cargo install --features embeddings --git https://github.com/TudorAndrei/criv criv
```

```toml
[index]
embeddings = true
```

Released binaries are built without the feature, so `--semantic` always fails on
them even when `criv.toml` enables it. This is deliberate, per
[ADR-0047](docs/adr/0047-semantic-note-search-stays-source-only.md): enabling
embeddings grows the binary from roughly 12 MB to roughly 30 MB, against a
release profile tuned for size.

The backend is not fully local. The first `--semantic` run downloads model
weights from the Hugging Face Hub and a prebuilt ONNX Runtime shared library,
populating a cache of roughly 97 MB under `.criv/embeddings`. The download is
silent. Later runs reuse the cache and need no network. With an empty cache and
no network, the command fails with an error retrieving `model.onnx`.

Every other criv command runs offline, which is what makes them safe inside git
hooks and CI. Semantic search is the one exception; keep it out of automated
gates unless the cache is known to be warm.

Run the same enforcement path used by hooks and CI:

```sh
criv enforce --stage commit
criv enforce --stage push
criv enforce --stage ci
```

## Reconciling branch-local ADR IDs

An ADR number is provisional until it exists on the integration target. When
two branches add the same next number, check the target allocation before
merging:

```sh
criv adr reconcile --base origin/main --check
```

The command prints the exact resolved target SHA and any deterministic mapping.
If it reports a collision, run the same command without `--check`; it renames
the branch-local ADRs, rewrites branch-owned references, validates the vault,
and writes an ignored `.criv/adr-reconcile.json` receipt. The receipt is local
commit proof for that exact generated transaction; it is not allocation state
and never makes a later reconciliation check succeed. Review and commit the
generated change:

```sh
criv adr reconcile --base origin/main
cargo run -- check
git add docs src # the receipt is ignored; do not add it
git add -u
git commit -m "docs(adr): reconcile branch-local ADR identities"
```

Before merging, compare the target SHA printed by the command with the current
target. If it moved, rerun reconciliation against the new SHA. The merge queue
or coordinator serializes this loop; criv does not reserve IDs, push, or merge.
`criv query next-adr-id` remains checkout-local and is not an allocation
authority.

## Reconciling ADR source governance

An exact source rename does not change an ADR's meaning, but its `governs:`
path still needs to follow the file. After committing the source rename, inspect
the repair against the integration target:

```sh
criv adr reconcile-sources --base origin/main --check
```

If a repair is required, run the command without `--check`. It accepts only
one-to-one Git-proven renames, rewrites exact `governs:` path scalars, validates
the vault, and creates a dedicated
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

`criv init` installs an Obsidian companion plugin scaffold under
`.obsidian/plugins/criv/` unless `--no-obsidian` is passed. The plugin is a UI
over `.criv/state.json`: it reads the CLI-generated graph state, validates the
schema version, renders source and pattern context, offers source autocomplete,
and delegates shared helper logic to the `criv-wasm` crate.

Build the plugin when working on its templates or WASM helper:

```sh
npm --prefix .obsidian/plugins/criv ci
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
`criv check --format json` diagnostics, and previews standalone `.c4` artifacts.

For local development:

```sh
npm --prefix extensions/vscode-criv install
npm --prefix extensions/vscode-criv run build
npm --prefix extensions/vscode-criv run test
npm --prefix extensions/vscode-criv run test:integration
```

The `.c4` preview command renders Mermaid C4 artifacts with Mermaid 11 and DOT
code artifacts with `@viz-js/viz` inside a webview. The text `.c4` file remains
the source of truth; the preview is only a projection.

## Releases

Release steps are documented in [docs/releasing.md](docs/releasing.md).

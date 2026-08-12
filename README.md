# criv

`criv` is a local CLI for keeping repository documentation connected to the code
it describes. It turns a repo's `docs/` folder into a validated knowledge graph
of notes, source references, architectural decisions, policy patterns, and local
state that other tools can read.

## What

`criv` gives a codebase an executable documentation layer:

- `criv init` creates the vault files, ADR directory, and agent skills.
- `criv check` validates documentation, source references, and ADR rules.
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
  adr/
.criv/
.agents/skills/
```

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
criv query next-adr-id
criv query coverage
criv query nodes --kind code --without-docs
criv query governs ADR-0001
criv query governing src/main.rs
criv query c4-code 'src/**'
criv query diff latest latest
```

See [docs/query-reference.md](docs/query-reference.md) for every `criv query`
subcommand, positional argument, flag, and example.

`c4-code` emits LikeC4 source with modules and imports for a focused source
glob. It does not emit files, classes, functions, methods, or calls.

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
and never makes a later reconciliation check succeed. The command creates the
dedicated commit itself. Review it and validate the result:

```sh
criv adr reconcile --base origin/main
cargo run -- check
git show --stat --oneline HEAD
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
source autocomplete, and delegates shared helper logic to the `criv-wasm`
crate.

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
`criv check --format json` diagnostics, and previews standalone `.c4` artifacts.

For local development:

```sh
npm ci
npm --prefix extensions/vscode-criv run build
npm --prefix extensions/vscode-criv run test
npm --prefix extensions/vscode-criv run test:integration
```

The default `.c4` editor renders the matching named LikeC4 view from
`.criv/state.json`. Use **Reopen Editor With → Text Editor** to edit the DSL.
The extension packages the renderer and its assets. It does not use a global
LikeC4 command or a network service. The `.c4` workspace remains the source of
truth; the preview is only a projection.

## Releases

Release steps are documented in [docs/releasing.md](docs/releasing.md).

<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/logo/wordmark-light.svg">
    <img src="assets/logo/wordmark-light.svg" width="224" alt="criv">
  </picture>
</h1>

criv connects your architectural decisions (ADRs) to the code they govern, and
blocks changes that break them.

## Why

Decision records drift because nothing checks them against the code. criv does:

- An ADR names the source paths it governs and links to files and symbols.
  `criv check` fails when a link no longer resolves.
- An ADR can carry ast-grep policies. `criv enforce` blocks the commit, push, or
  CI run that breaks one.
- `criv query` answers which decisions govern a file, and which code has no
  decision.

criv runs locally, with no server and no network access.

## Install

Add criv to the `mise.toml` of your repository:

```toml
[tools."github:TudorAndrei/criv"]
version = "latest"
```

Then run:

```sh
mise install
criv init
```

`criv init` writes `criv.toml`, the `docs/adr/` folder, and agent skills. Keep
`.criv/` out of Git.

## Use

```sh
criv adr new "Cache compiled policies"   # draft a decision
criv check                               # validate decisions and their links
criv enforce --stage commit              # block changes that break a policy
criv query governing src/main.rs         # which decisions govern this file?
```

An ADR declares its scope and policies in its frontmatter:

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

Each diagnostic prints the `fix:` that repairs it. Run `criv <command> --help`
for every flag, or `criv --usage-json` for the full command tree.

## Learn more

- [Hook runner setup](docs/tooling.md#running-criv-from-a-hook-runner) for hk
  and lefthook.
- [Configuration](docs/configuration.md), [queries](docs/query-reference.md),
  and [local State](docs/state-reference.md).
- [Elixir source support](docs/elixir-support.md).
- Editor viewer: `criv install-editor --editor code`.
- Architecture diagrams and Gherkin integration live in
  [Concord](https://github.com/TudorAndrei/concord).
- [Decision index](docs/adr/README.md).

## Develop

See [AGENTS.md](AGENTS.md), [tooling](docs/tooling.md),
[testing](docs/testing.md), and [releasing](docs/releasing.md).

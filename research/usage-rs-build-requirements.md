# usage-rs Build and Dependency Requirements

Date: 2026-08-22

## Question

What does `usage-rs` 6.0.0 require of the criv build, and what does it cost?

## Answer

criv needs the default features only: `spec`, `help`, and `diagnostics`. Add
`test` to the dev-dependencies, and add `completions` only if criv publishes
shell completion scripts. criv publishes none today.

There is no toolchain conflict. `usage-rs` uses edition 2021 and asks for Rust
1.91. criv uses edition 2024 and pins Rust 1.97.1. Cargo compiles each crate
with its own edition. The measured builds prove this.

`usage-lib` leaves the release binary. The derive replaces it. Keep
`usage-lib` only as an optional dev-dependency for the KDL round-trip test.

The migration removes 28 crates from the criv dependency graph and 299,600
bytes from the stripped release binary. The measurements are below.

One constraint blocks a larger saving: `rumdl` 0.2.41 requires `clap` and
`clap_complete` without a feature gate. Therefore the clap crates stay in the
criv build after the migration.

This research reads `usage-rs` 6.0.0, published to crates.io on 2026-08-22, and
the `jdx/usage` repository at commit
[`88d75950`](https://github.com/jdx/usage/tree/88d759504e387f6c07b5ab47d42011f0cfcd112f).

## 1. Cargo features

The published manifest declares this feature table. It agrees with the
repository manifest, character for character.

| Feature       | Default | What it gives                                        |
| ------------- | :-----: | ---------------------------------------------------- |
| `spec`        |   yes   | Spec metadata, `to_kdl()`, and the derive macros     |
| `help`        |   yes   | The `-h` and `--help` pages                          |
| `diagnostics` |   yes   | clap-shaped error text from `render_failure`         |
| `completions` |   no    | Completion scripts and the completion protocol       |
| `validation`  |   no    | Portable `validate` and `validate_error` expressions |
| `test`        |   no    | `usage::test` assertions, for dev-dependencies       |
| `config`      |   no    | The `usage::Config` derive and the settings resolver |

Source: the published crate manifest
[`usage-rs/Cargo.toml`](https://github.com/jdx/usage/blob/88d759504e387f6c07b5ab47d42011f0cfcd112f/usage-rs/Cargo.toml)
and the
[Rust framework page](https://usage.jdx.dev/rust/).

criv must enable the defaults only:

```toml
[dependencies]
usage = { package = "usage-rs", version = "6.0.0" }

[dev-dependencies]
usage = { package = "usage-rs", version = "6.0.0", features = ["test"] }
```

The reasons for each opt-in feature:

- `completions`: not required. criv has no completion command and no
  `clap_complete` use of its own. The `clap_complete` crate in the graph belongs
  to `rumdl`. Add this feature only if the migration also adds completions. The
  `usage-test` completion assertions in map #185 need it too.
- `validation`: not required. criv uses one `conflicts_with` in `src/check.rs`
  and two `requires` in `src/enforce.rs`. usage supports these relationships in
  the core. The `validation` feature covers portable expressions only.
- `config`: not required. Map #185 puts `usage::Config` for `criv.toml` out of
  scope.
- `test`: required for the parity evidence in map #185. Keep it in
  `dev-dependencies`. Cargo resolver 3, which edition 2024 selects, does not
  give dev-dependency features to the release binary.

## 2. Edition and Rust version

| Crate      | Edition | `rust-version` |
| ---------- | ------- | -------------- |
| `usage-rs` | 2021    | 1.91           |
| `criv`     | 2024    | 1.97.1         |

There is no conflict. Cargo compiles each crate with the edition in its own
manifest. A dependency with a lower `rust-version` is always satisfied by a
higher toolchain. The rule is one-way: a dependency must not ask for more than
the toolchain gives.

This is proven, not assumed. Every measurement in this document used
`rustc 1.97.1 (8bab26f4f 2026-07-14)`, the version that `mise.toml` and the
release workflow pin. All builds finished with exit code 0.

`usage-lib` 6.0.0 asks for Rust 1.95, which is also below criv's pin.

## 3. What happens to `usage-lib`

criv depends on `usage-lib` for two jobs:

1. `src/lib.rs:192` renders every help page with
   `usage::docs::cli::render_help`.
2. `src/lib.rs:179` derives a spec from the clap command with
   `(&Cli::command()).into()`, and `src/lib.rs:175` prints it for `--usage`.

`usage-rs` does both jobs with its default features.

- Help. The parser supplies `-h` and `--help`. `usage::help::render` renders a
  page from the static spec. The upstream documentation states that the output
  matches what `usage-lib` renders from the same spec, and that the two
  renderers are held to identical output over 211 mise command pages in CI. See
  [Help, version, and errors](https://usage.jdx.dev/rust/help).
- Spec. `Cli::to_kdl()` writes the spec in process. Every binary also answers
  the `__usage_spec__` word. See
  [Spec output](https://usage.jdx.dev/rust/spec).

So `usage-lib` leaves the release binary. It has one remaining use: the
round-trip test that upstream recommends to every adopter. That test parses
`Cli::to_kdl()` with `usage-lib` and belongs in `dev-dependencies`:

```toml
[dev-dependencies]
usage-parser = { package = "usage-lib", version = "6.0.0" }
```

This dev-dependency is optional. It costs nothing in the release binary. Note
that `usage-lib` moved from 3.5.6 to 6.0.0, so this is also a major upgrade.

criv's manifest asks for `usage-lib` 3.5.3. The lock file resolves 3.5.6. All
measurements below use the locked 3.5.6.

## 4. Measured cost

### Method

Every build used the criv release profile without a change:
`opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`,
`strip = true`. The host is macOS on `aarch64-apple-darwin`. The command was
`cargo build --release -p criv --bin criv`. All scratch edits were reverted.

Three criv builds:

- **A. Today.** criv without a change: `clap` 4 plus `usage-lib` 3.5.6.
- **B. Without `usage-lib`.** The `usage-lib` dependency removed, and the help
  and spec functions replaced by stubs. clap still parses.
- **C. With `usage-rs`.** Build B plus `usage-rs` 6.0.0 at its defaults. A
  generated declaration of criv's shape drives it: 7 top-level commands, a
  14-variant query enum, a 4-variant adr enum, 16 `Args` structs, 21 long flags,
  and 6 value enums. `write_usage_spec` calls `to_kdl()`. `usage_help` calls
  `parse_from`, `help::render`, and `render_failure`. The linker therefore keeps
  the parser, the help renderer, the diagnostics, and the KDL writer.

Build C keeps clap, because criv's own code still parses with it. Build C is
therefore an upper bound on the end state.

### Release binary size

| Build                             | Bytes      | Change from A |
| --------------------------------- | ---------: | ------------: |
| A. criv today                     | 12,168,992 |             — |
| B. `usage-lib` removed            | 11,685,584 |      -483,408 |
| C. B plus `usage-rs` 6.0.0        | 11,869,392 |      -299,600 |

- `usage-lib` 3.5.6 costs **483,408 bytes** (472.1 KiB) in criv today.
- `usage-rs` 6.0.0 costs **183,808 bytes** (179.5 KiB) in criv.
- The net change is **-299,600 bytes** (-292.6 KiB), or **-2.46 %**.

Build times, for reference: A took 1 min 45 s, B took 1 min 14 s, and C took
47 s. The target directory was warm.

### Dependency graph

Counted with `cargo tree -p criv --edges normal --target aarch64-apple-darwin`,
with duplicates removed. The count includes the `criv` package itself.

| Build                      | Normal crates | All crates |
| -------------------------- | ------------: | ---------: |
| A. criv today              |           180 |        196 |
| B. `usage-lib` removed     |           149 |          — |
| C. B plus `usage-rs` 6.0.0 |           152 |        168 |

Removing `usage-lib` 3.5.6 removes 31 crates:

```text
chacha20, duct, either, filetime, homedir, itertools 0.14, itertools 0.15,
kdl, miette, miette-derive, nix, nom, num-traits, os_pipe, rand, rand_core,
roff, shared_child, shared_thread, shell-words, sigchld, signal-hook,
signal-hook-registry, strum, strum_macros, tera, unicode-width, usage-lib,
versions, winnow, xx
```

Adding `usage-rs` 6.0.0 adds exactly 3 crates:

```text
usage-rs, usage-argv, usage-derive
```

`usage-argv` has no dependencies at all. `usage-derive` uses `proc-macro2`,
`quote`, and `syn` 3 only. criv already builds all three: `syn` 3.0.3 arrives
with `clap_derive` 4.6.4. So the migration adds no new third-party crate.

The net change is **-28 crates**.

### Isolated comparison

criv shares many crates between its dependencies, so the criv numbers understate
the difference between the two CLI stacks. Four small binaries measure the
stacks alone. All four use the criv release profile and the same generated CLI
shape.

| Binary                                          |     Bytes | Stack cost |
| ----------------------------------------------- | --------: | ---------: |
| Empty `main`, no CLI crate                      |   286,064 |          — |
| `clap` 4.6.6 with `derive` and `env`            |   436,384 |    150,320 |
| `usage-rs` 6.0.0, defaults, parse and `to_kdl`  |   503,536 |    217,472 |
| `clap` plus `usage-lib` 3.5.6, help and spec    | 1,782,976 |  1,496,912 |

The bottom row is criv's stack today. The row above it is the stack after the
migration. In a standalone binary the swap saves **1,279,440 bytes**, or 85.5 %
of the CLI stack.

### The clap constraint

criv cannot remove the clap crates. `rumdl` 0.2.41 declares `clap` (with
`derive`) and `clap_complete` as plain dependencies, with no feature gate. criv
already sets `default-features = false` on `rumdl`, and clap still arrives.

Twelve clap-family crates therefore stay in the graph after the migration:

```text
anstream, anstyle, anstyle-parse, anstyle-query, clap, clap_builder,
clap_complete, clap_derive, clap_lex, colorchoice, is_terminal_polyfill, strsim
```

Link-time optimization can still remove clap code that nothing calls. The
isolated table above puts criv's own clap surface near 150 KB. The end-state
binary is therefore near 11.72 MB. That last figure is an estimate, because a
real number needs the finished migration.

### What was not measured

The end-state binary itself. A true measurement needs all 16 `Args` structs
rewritten, which map #185 puts outside this research. Build C links the complete
`usage-rs` runtime against a declaration of the same shape, so the runtime cost
is real. Only the static tables can differ, and they are a small part of the
total.

## 5. Release matrix targets

The release workflow `.github/workflows/release.yml` builds four targets with
toolchain 1.97.1:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

`cargo check --target <target>` on the `usage-rs` probe crate passed on all
four, with Rust 1.97.1. `aarch64-apple-darwin` also passed a full release build
and ran the binary.

The result is expected. `usage-argv` has no build script, no dependency, and no
platform-specific crate. Upstream states that a non-UTF-8 `PathBuf` or
`OsString` argument keeps its bytes on Unix, and that Windows reports a value it
cannot convert safely. See
[Current limitations](https://usage.jdx.dev/rust/).

## Conclusion

The build cost is small and the saving is real.

1. Enable the defaults. Add `test` to `dev-dependencies`. Add `completions` only
   with a completion command.
2. The edition and Rust version give no conflict. Builds prove it.
3. Remove `usage-lib` from `dependencies`. Keep it as an optional
   `dev-dependency` at 6.0.0 for the KDL round-trip test.
4. The migration removes 28 crates and 299,600 bytes from the criv release
   binary.
5. All four release targets compile.

No blocker stops the migration. One limit shapes the expected saving: `rumdl`
keeps clap in the criv build.

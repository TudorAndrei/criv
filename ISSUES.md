# criv Audit Issues

Audit date: 2026-06-20
Audited commit: `e90241f`

This file records the vetted findings from the June 2026 read-only improvement
audit. It is intentionally an issue index, not an implementation plan. Convert a
finding into a focused plan before editing production code.

## Verification Baseline

The audit verified the current baseline with these commands:

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `npm --prefix .obsidian/plugins/criv test`
- `npm --prefix .obsidian/plugins/criv run lint`
- `npm --prefix .obsidian/plugins/criv run format:check`
- `npm --prefix .obsidian/plugins/criv audit --audit-level=high --omit=dev`

All commands above passed. `cargo audit` was not installed locally, so Rust
advisory coverage remains unverified.

## Planning Order

Address issues 1, 2, 3, and 4 first.

- Issue 1 should land before any plugin trust-model work.
- Issue 2 should land before expanding watcher behavior.
- Issue 3 is independent and small.
- Issue 4 can land with issue 2 because both touch watch/state write behavior.

## Issue 1: Confine Configured Paths To The Vault Root

Category: Security / Correctness
Effort: M
Fix risk: MED
Confidence: HIGH
Status: Resolved in `f59823b` (`fix(config): confine vault-owned paths`).

`criv.toml` can currently steer file reads and generated writes outside the
vault root because config strings are joined directly onto the root path.

Evidence:

- `src/config.rs:75` builds `docs_path` with `root.join(&self.docs_dir)`.
- `src/config.rs:79` maps each source root with `root.join(source_root)`.
- `src/architecture.rs:23` writes `[architecture.code].output` through
  `root.join(&config.output)`.
- `src/vault.rs:549` walks configured source roots after that join.

Related decisions:

- `docs/adr/0001-local-cli-vault-architecture.md` defines a vault as rooted by
  `criv.toml`, `docs/`, `docs/adr/`, and local `.criv/` state.
- `docs/adr/0021-audit-remediation-boundaries.md` says user-facing config
  fields should correspond to active, behavior-backed validation.
- `docs/adr/0009-obsidian-plugin-as-state-consumer.md` makes the plugin a
  consumer of CLI-generated local state, so unsafe source paths can flow into
  plugin previews.

Impact:

A repository can configure `docs`, `source.roots`, or `architecture.code.output`
with absolute paths or `..` segments. Running `criv watch --once` in such a
repo can read source outside the vault or write generated architecture outside
the repo. This matters for users who run criv in cloned or otherwise untrusted
repositories.

Fix sketch:

Introduce a small path-normalization helper near config loading or `util.rs`.
It should reject absolute paths and parent-directory traversal for vault-owned
paths. Apply it to `vault.docs`, `vault.adr`, `source.roots`, and
`architecture.code.output`; keep glob patterns such as `source.exclude` as
patterns, not filesystem paths.

Verification:

- Add config unit tests for absolute paths, `..`, and normal relative paths.
- Add a CLI workflow test showing an escaping architecture output is rejected.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- check`.

## Issue 2: Write State And Snapshot Pointers Atomically

Category: Correctness / DX
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Resolved in `01cacd0` (`fix(watch): serialize state refreshes`).

The state writer writes `.criv/state.json`, snapshot files, and `.criv/latest`
directly to their final paths.

Evidence:

- `src/state.rs:383` writes `.criv/state.json`.
- `src/state.rs:393` writes content-addressed snapshots and `.criv/latest`.
- `src/watch.rs:104` writes state during rebuild.
- `.obsidian/plugins/criv/src/main.ts:191` reads `.criv/state.json` in the
  plugin.

Related decisions:

- `docs/adr/0007-content-addressed-state-and-diffing.md` makes `src/state.rs`
  own `.criv/state.json`, snapshots, and snapshot pointers.
- `docs/adr/0009-obsidian-plugin-as-state-consumer.md` says Obsidian reads the
  generated state and validates schema compatibility.

Impact:

Long-running `criv watch` can update state while Obsidian or another consumer is
reading it. A direct final-path write allows a transient partial file, which can
produce parse failures or stale UI until the next reload.

Fix sketch:

Add an atomic write helper that writes to a temporary file in the destination
directory, flushes it, and renames it over the final path. Use it for
`.criv/state.json`, new snapshot files, and `.criv/latest`.

Verification:

- Add unit tests around the helper where practical.
- Add a state writer test asserting the same final files still exist and parse.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- watch --once`.
- Run `cargo run --quiet -- check`.

## Issue 3: Use Real JSON Serialization For CLI JSON Output

Category: Correctness / DX
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Resolved in `befef3c` (`fix(cli): serialize json output with serde`).

The JSON output paths manually escape strings. They handle quotes, backslashes,
and sometimes newlines, but not the full JSON string surface.

Evidence:

- `src/query.rs:542` prints JSON rows by hand.
- `src/query.rs:560` escapes only backslashes and quotes.
- `src/search.rs:481` prints JSON rows by hand.
- `src/check.rs:981` prints diagnostic JSON by hand.

Impact:

`--format json` can emit invalid JSON when row values contain tabs, carriage
returns, control characters, or other values that `serde_json` would escape
correctly. That weakens criv as a CLI surface for agents and editor tooling.

Fix sketch:

Replace hand-built JSON printing with small serializable structs and
`serde_json::to_writer_pretty` or `serde_json::to_string_pretty`. Preserve the
current text output unchanged.

Verification:

- Add tests for `query --format json`, `search --format json`, and
  `check --format json` with strings containing quotes, tabs, and newlines.
- Parse command output with `serde_json::from_slice` in integration tests.
- Run `cargo test --workspace`.

## Issue 4: Acquire The Watch Lock Before Startup Rebuild

Category: Correctness / Concurrency
Effort: M
Fix risk: LOW
Confidence: HIGH
Status: Resolved in `01cacd0` (`fix(watch): serialize state refreshes`) for
startup rebuild serialization. Stale lock detection was not added; `watch
--once` remains lock-free.

`criv watch` performs the initial rebuild before it acquires `.criv/watch.lock`.

Evidence:

- `src/watch.rs:24` starts `run`.
- `src/watch.rs:25` calls `rebuild(root, None)` before lock acquisition.
- `src/watch.rs:31` acquires the watch lock only after the rebuild.
- `src/watch.rs:137` implements the current lock file with `create_new(true)`.

Related decisions:

- `docs/adr/0006-fff-source-index-and-incremental-watch.md` keeps watch mode
  responsible for coordinating docs and source rebuilds.
- `docs/adr/0007-content-addressed-state-and-diffing.md` routes state and
  snapshot updates through watch rebuilds.

Impact:

Two `criv watch` processes can both write generated architecture or state during
startup before one fails to acquire the lock. A crashed process can also leave a
stale lock that blocks later watch runs.

Fix sketch:

Acquire the lock before the initial rebuild for long-running watch mode. Keep
`watch --once` lock-free unless a separate decision says one-shot rebuilds must
be serialized. Consider storing the process id in the lock file and detecting
stale locks if that can be done portably.

Verification:

- Add a test or small internal seam that proves lock acquisition happens before
  rebuild in long-running mode.
- Add coverage for stale lock messaging if stale detection is added.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- watch --once`.

## Issue 5: Sanitize Or Constrain DOT SVG Preview Output

Category: Security
Effort: M
Fix risk: MED
Confidence: MED
Status: Resolved in `680c5cd` (`fix(obsidian): harden dot preview rendering`).
The `@viz-js/viz` probe showed DOT `URL="javascript:..."` emits an SVG
`xlink:href`, so the plugin now sanitizes DOT SVG before `innerHTML`.

The Obsidian DOT preview inserts rendered SVG directly into the DOM.

Evidence:

- `.obsidian/plugins/criv/src/main.ts:913` defines the DOT renderer.
- `.obsidian/plugins/criv/src/main.ts:923` assigns `result.output` to
  `container.innerHTML`.
- `.obsidian/plugins/criv/src/core.ts:213` classifies `.c4` artifacts and allows
  DOT as a supported format.

Related decisions:

- `docs/adr/0009-obsidian-plugin-as-state-consumer.md` makes the plugin a
  maintained state consumer, not a disposable sample.
- `docs/adr/0030-dot-for-generated-code-architecture.md` records DOT as the
  generated code architecture format.
- `docs/adr/0032-c4-files-as-architecture-artifacts.md` makes standalone `.c4`
  artifacts normal vault content.

Impact:

A malicious `.c4` DOT file may be able to produce unsafe SVG content or links in
Obsidian. Mermaid rendering is initialized with strict security settings, but
the DOT output path does not apply an equivalent sanitization boundary.

Fix sketch:

Investigate the SVG output surface from `@viz-js/viz`. If unsafe nodes or
attributes are possible, sanitize the SVG before insertion or render it through
a safer embedding path. Keep fallback source rendering behavior unchanged.

Verification:

- Add plugin tests for DOT content containing labels, URLs, and suspicious SVG
  payload attempts.
- Run `npm --prefix .obsidian/plugins/criv test`.
- Run `npm --prefix .obsidian/plugins/criv run lint`.
- Run `npm --prefix .obsidian/plugins/criv run build`.

## Issue 6: Avoid Whole-Repo Source Index Startup For Narrow Roots

Category: Performance
Effort: M
Fix risk: MED
Confidence: MED
Status: Resolved in `d022249` (`perf(source-index): respect configured roots
during startup`). `fff-search` exposes one `base_path`, so criv now starts one
picker per configured directory root and handles configured file roots directly.
Current local `mise run perf` result after the change: `source_index_files`
`real=1.68s`.

The fff-backed source index starts at the repository root and filters to
configured roots after the scan.

Evidence:

- `src/source_index.rs:69` starts `FilePicker` with `base_path: root`.
- `src/source_index.rs:111` applies source-root filtering after indexing.
- `src/vault.rs:546` has a separate source-file collector that walks configured
  roots directly.

Related decisions:

- `docs/adr/0006-fff-source-index-and-incremental-watch.md` expects source
  indexing and watch rebuilds to avoid unnecessary recomputation.
- `docs/tooling.md` documents `mise run perf` for changes to source indexing and
  watch behavior.

Impact:

Large repositories with narrow `[source].roots` still pay startup cost for a
whole-repo fff scan. This undermines `source.roots` as a performance boundary.

Fix sketch:

Investigate whether `fff-search` supports multiple roots or a narrower base
path. If it does, make `FffSourceIndex` scan only the configured roots. If not,
record the limitation and consider a lightweight direct file list for entries
while keeping fff for fuzzy search and grep.

Verification:

- Add tests for source roots that are files, directories, and hidden paths.
- Run `cargo test --workspace`.
- Run `mise run perf` before and after the change on this repo and on a larger
  vault if available.

## Issue 7: Pin The Release Rust Toolchain

Category: DX / Release
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Resolved in `4c6552e` (`ci(release): pin rust toolchain`).

The release workflow builds with moving `stable` while local development and CI
pin Rust `1.95.0`.

Evidence:

- `mise.toml:1` pins `rust = "1.95.0"`.
- `.github/workflows/ci.yml:32` installs Rust components for toolchain
  `1.95.0`.
- `.github/workflows/release.yml:32` installs and selects `stable`.

Related decisions:

- `docs/adr/0022-hosted-ci-entry-point.md` keeps hosted CI aligned with the
  repository validation entry point.
- `docs/adr/0014-tag-triggered-release-binary-workflow.md` owns release binary
  workflow behavior.
- `docs/adr/0015-size-optimized-release-profile.md` makes release binary output
  a deliberate optimization surface.

Impact:

Release tags may fail or produce subtly different binaries when `stable` moves
ahead of the pinned local and CI toolchain. That creates release-only risk after
normal checks have passed.

Fix sketch:

Use the same Rust version in release CI as `mise.toml`, either by installing
`1.95.0` directly or by using mise in the release workflow. Keep target
installation and smoke tests unchanged.

Verification:

- Run `actionlint` after editing workflow YAML.
- Run `zizmor --offline --strict-collection .`.
- Run `cargo build --locked --release --bin criv` locally if practical.

## Direction Options

These are product or architecture options, not bugs.

1. Build a shared conformance suite for Rust and plugin parsing. The TypeScript
   plugin duplicates C4 artifact and link logic from Rust, with only partial
   shared fixtures today. This would reduce drift as C4 support expands.
2. Make machine-readable CLI output first-class. `query` currently returns
   strings even in JSON mode, while `search` and `check` have separate ad hoc
   serializers. A typed output layer would make criv easier for agents and
   editor integrations to consume.
3. Define an explicit trust model for vault execution. The project now spans
   local CLI config, generated state, and Obsidian rendering. A short
   trusted-repo policy would guide fixes like path confinement, SVG
   sanitization, and external editor URLs.

## Considered And Rejected

- `git show` usage in `query diff` and enforcement reads is passed as process
  arguments, not shell-built, so this audit did not find command injection
  there.
- WASM state summarization handles parse errors at the exported boundary, so the
  internal test `unwrap()` is not a production panic.

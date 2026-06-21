# criv Audit Issues

Audit date: 2026-06-21
Audited commit: `56e1bce`

This file records vetted findings from a read-only improvement audit. It is an
issue index, not an implementation plan. Convert a finding into a focused plan
before editing production code.

## Verification Baseline

The audit verified the current baseline with these commands:

- `cargo test --workspace`
- `npm --prefix .obsidian/plugins/criv test`
- `npm --prefix .obsidian/plugins/criv audit --audit-level=high --omit=dev`

The Rust and plugin test commands passed. The npm production advisory check
reported `found 0 vulnerabilities`.

`cargo audit` was not installed locally, so Rust advisory coverage remains
unverified.

## Planning Order

Address issue 1 first. It affects source graph identity and therefore generated
state, generated architecture, source queries, and C4 interface anchors.

- Issue 1 should land before expanding generated code architecture or C4
  interface drift behavior.
- Issue 2 is independent and small, but should land before treating
  `watch --once` as a fully serialized state refresh.
- Issue 3 is independent release tooling work.
- Issue 4 is an independent verification baseline improvement.
- Issue 5 should land before larger Obsidian plugin UI work.
- Issue 6 should land before recommending code references as editor-friendly in
  Obsidian-heavy workflows.

## Issue 1: Use Collision-Free Source Symbol IDs

Category: Correctness / Architecture
Effort: M
Fix risk: MED
Confidence: HIGH
Status: Open

Source symbol identity is currently only `path#name`, which collides for methods
or constructors that share a name inside the same file.

Evidence:

- `src/source_graph.rs:51` defines `SymbolId` with only `path` and `name`.
- `src/source_graph.rs:57` renders IDs as `path#name`.
- `src/state.rs:120` converts `symbol.id.display()` into graph node IDs.
- `src/state.rs:753` deduplicates graph nodes by ID, so colliding symbols are
  merged or dropped.
- `src/c4_code.rs:67` uses the same symbol display string as generated DOT node
  identity.
- `.obsidian/plugins/criv/src/main.ts` has multiple same-name methods in one
  file, including `render`, `constructor`, `getViewType`, and `onOpen`.

Impact:

The generated graph state, generated DOT code architecture, `query callers`,
`query callees`, and C4 interface anchors can all refer to the wrong same-file
symbol or collapse several symbols into one node. This is already visible in
the repository's own generated code node list, where
`.obsidian/plugins/criv/src/main.ts#render` and
`.obsidian/plugins/criv/src/main.ts#constructor` appear multiple times.

Fix sketch:

Introduce a stable, collision-free symbol identity that includes enough scope to
distinguish same-file symbols. For example, include class/impl parent and, only
where still needed, a semantic disambiguator. Following ADR-0034, expose this
as an AST-aware source selector rather than a plain `path#name` anchor, raw
tree-sitter node path, or line-based selector. Update symbol resolution, state
node IDs, generated DOT IDs, C4 `criv:source` anchors, governance targets, and
C4 interface hash lookup together. Keep human-facing labels readable, but make
machine IDs unique.

Verification:

- Add source graph tests for two same-name methods in different classes in one
  file.
- Add state tests asserting both methods produce distinct graph nodes.
- Add generated DOT tests for same-file same-name methods.
- Add C4 source-anchor tests that can target the intended method unambiguously.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- watch --once`.
- Run `cargo run --quiet -- check`.

## Issue 2: Serialize `watch --once` With The Watch Lock

Category: Correctness / Concurrency
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The long-running watch mode acquires `.criv/watch.lock`, but `watch --once`
returns through the one-shot rebuild path before any lock acquisition.

Evidence:

- `src/watch.rs:24` starts `watch::run`.
- `src/watch.rs:25` calls `rebuild(root, None)` and returns immediately for
  `--once`.
- `src/watch.rs:29` acquires `WatchLock` only for long-running watch mode.
- `tests/cli_workflows.rs:581` covers long-running watch lock behavior, but not
  one-shot lock behavior.
- `README.md` documents `criv watch --once` as the state writer used by hooks
  and Obsidian refresh workflows.

Impact:

Hook, editor, or manual `watch --once` runs can overlap with a long-running
watch process. Atomic state writes reduce partial-read risk, but generated
architecture writes and state rebuilds can still interleave.

Fix sketch:

Acquire the same watch lock around the one-shot rebuild path. If another watch
process owns the lock, fail clearly: an active watcher already owns state
refresh, so running `watch --once` is unnecessary and should not race it. Keep
`criv check` as validation-only behavior; it should report vault problems, not
silently replace a blocked state refresh. If stale-lock behavior is in scope,
record the PID in the lock file and treat stale detection as a separate, tested
behavior; do not silently delete locks without a clear policy.

Verification:

- Add a CLI workflow test that pre-creates `.criv/watch.lock`, runs
  `criv watch --once`, expects failure, and verifies `.criv/state.json` is not
  changed.
- Keep the existing long-running lock test passing.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- watch --once` in a normal repo state.

## Issue 3: Always Add The Target Plugin Version To `versions.json`

Category: Release / Correctness
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The Obsidian plugin version helper skips adding the new plugin version when the
minimum app version is already present as any existing value.

Evidence:

- `.obsidian/plugins/criv/version-bump.mjs:3` reads
  `process.env.npm_package_version` as the target plugin version.
- `.obsidian/plugins/criv/version-bump.mjs:10` checks
  `Object.values(versions).includes(minAppVersion)` instead of checking whether
  `versions[targetVersion]` exists.
- `.obsidian/plugins/criv/package.json:11` wires this script into the npm
  `version` lifecycle.
- `.obsidian/plugins/criv/versions.json:1` currently maps plugin `0.1.0` to
  Obsidian `1.5.0`.

Impact:

Normal plugin releases often keep the same `minAppVersion`. In that case,
`versions.json` will not get an entry for the new plugin version, which can
break Obsidian plugin release metadata even though `manifest.json` was bumped.

Fix sketch:

Change the script to set `versions[targetVersion] = minAppVersion` whenever the
target key is missing or has a different value. Add a small Node test or script
unit seam that verifies a new target version is added even when another version
already has the same `minAppVersion`.

Verification:

- Add plugin test coverage for the version bump behavior, preferably without
  mutating the real manifest files.
- Run `npm --prefix .obsidian/plugins/criv test`.
- Run `npm --prefix .obsidian/plugins/criv run lint`.
- Run `npm --prefix .obsidian/plugins/criv run format:check`.

## Issue 4: Add Rust Advisory Scanning To The Verification Baseline

Category: Security / Dependencies
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The repository has npm advisory coverage available for plugin production
dependencies, but no Rust advisory gate in the documented check path.

Evidence:

- `Cargo.toml:20` lists Rust dependencies that ship in the CLI and WASM helper.
- `hk.pkl:65` defines the repository `check` hook without a Rust advisory step.
- `.github/workflows/ci.yml:36` runs `mise run check`, so CI inherits the same
  omission.
- Running `cargo audit --version` locally failed because `cargo-audit` is not
  installed.

Impact:

Rust dependencies can gain known security advisories while `mise run check` and
CI remain green. This weakens release confidence for a local CLI that processes
repository content and invokes native tooling.

Fix sketch:

Choose a Rust advisory tool, such as `cargo-audit` or `cargo-deny`, pin it in
`mise.toml`, and add a check-only step to `hk.pkl` and CI through
`mise run check`. Document any accepted advisories explicitly if needed.

Verification:

- Run the chosen advisory command locally.
- Run `mise run check`.
- Confirm CI installs the tool through mise and runs the advisory gate.

## Issue 5: Add Direct Tests For Obsidian Plugin UI Behavior

Category: Test Coverage
Effort: M
Fix risk: LOW
Confidence: HIGH
Status: Open

The plugin test suite bundles and tests `src/core.ts`, but the UI-heavy
`src/main.ts` behavior is not directly covered.

Evidence:

- `.obsidian/plugins/criv/test/core.test.mjs:13` bundles only
  `.obsidian/plugins/criv/src/core.ts`.
- `.obsidian/plugins/criv/package.json:13` runs only
  `node test/core.test.mjs`.
- `.obsidian/plugins/criv/src/main.ts:183` reads `.criv/state.json`.
- `.obsidian/plugins/criv/src/main.ts:292` reads linked source previews.
- `.obsidian/plugins/criv/src/main.ts:399` patches Obsidian's native save
  command.
- `.obsidian/plugins/criv/src/main.ts:891` owns Mermaid and DOT C4 preview
  rendering.

Impact:

Recent high-value plugin behavior, including C4 rendering, source previewing,
save interception, hover state, and settings, can regress while the current
plugin test command stays green. Security-sensitive rendering fixes are covered
at the sanitizer helper level, but not at the renderer integration boundary.

Fix sketch:

Add a small DOM-capable test layer for `src/main.ts` behavior. Keep it focused:
mock enough of the Obsidian API to test state loading failures, preview reads,
save command patching, and DOT/Mermaid fallback behavior. Avoid full Electron or
Obsidian integration unless this lightweight seam proves insufficient.

Verification:

- Add tests under `.obsidian/plugins/criv/test/`.
- Run `npm --prefix .obsidian/plugins/criv test`.
- Run `npm --prefix .obsidian/plugins/criv run lint`.
- Run `npm --prefix .obsidian/plugins/criv run format:check`.
- Run `npm --prefix .obsidian/plugins/criv run build` if the test adds new
  imports or exports.

## Issue 6: Detect Obsidian-Broken Source Wiki-Links

Category: DX / Docs Correctness
Effort: M
Fix risk: MED
Confidence: HIGH
Status: Open

`criv check` accepts bare wiki-links that resolve to source files, but Obsidian
still marks links such as `[[src/structural.rs]]` and `[[src/search.rs]]` as
links to non-existent documents. ADR-0034 supersedes the earlier typed
source-Wikilink direction: Wikilinks should be reserved for document references,
and code references should use AST-aware source selectors wherever possible.

Evidence:

- A 2026-06-21 Obsidian screenshot showed source wiki-links underlined with
  errors like `Link to non-existent document 'src/structural.rs'` and
  `Link to non-existent document 'src/search.rs'`.
- `src/check.rs:822` validates wiki-links through `validate_links`.
- `src/check.rs:832` treats `ResolvedLink::Source` as valid, warning only when
  source resolution is ambiguous.
- `docs/adr/0020-portable-note-wikilinks.md:23` records the same editor
  portability problem for metadata-only note links, but its decision only covers
  note references.
- `docs/adr/0033-typed-wikilink-source-references.md` records typed
  `source:` Wikilinks as an earlier source-reference direction.
- `docs/adr/0034-ast-aware-source-selectors.md` supersedes ADR-0033 and records
  AST-aware selectors as the target model for code references.
- `docs/adr/0026-mermaid-c4-diagrams-as-vault-content.md:35` uses source
  wiki-links such as `[[src/c4.rs]]`, which criv understands but Obsidian can
  still report as missing documents.
- `.obsidian/plugins/criv/src/main.ts:263` decorates rendered links when the
  plugin has state, but that does not make Obsidian's native editor link
  diagnostics treat source wiki-links as existing files.

Impact:

Authors can follow legacy criv source-link examples and still see red Obsidian
broken-link diagnostics. That weakens trust in both criv checks and the vault
editor experience, especially in docs that deliberately link to source files
rather than Markdown notes.

Fix sketch:

Implement ADR-0034. Define an AST-aware source selector grammar for code
targets, then use it consistently for source governance, source anchors, source
graph IDs, query targets, generated architecture references, and source
references in note prose. Keep legacy bare source wiki-links, `path#name`
anchors, and ADR-0033 `source:` Wikilinks resolving during a compatibility
window. Add `criv check` diagnostics that guide document references toward
Wikilinks and code references toward AST-aware selectors. If authoring
ergonomics still require it, extend the Obsidian plugin's editor integration so
code references are decorated, previewed, and not confused with missing note
links.

Verification:

- Add a check test with `[[src/structural.rs]]` or an equivalent fixture source
  link that currently resolves through criv but is not editor-portable.
- Add tests showing AST-aware selectors resolve for files and symbols without
  relying on line or range identity.
- Assert the new diagnostic code and suggested replacement for bare source
  wiki-links, plain `path#name` anchors, and ADR-0033 `source:` Wikilinks.
- If plugin-side editor support is added, add plugin UI/editor tests that prove
  code references are decorated or resolved in edit mode, not only rendered
  preview mode.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- check`.
- If plugin code changes, run `npm --prefix .obsidian/plugins/criv test`,
  `npm --prefix .obsidian/plugins/criv run lint`, and
  `npm --prefix .obsidian/plugins/criv run format:check`.

## Direction Options

These are product or architecture options, not bugs.

1. Build a shared Rust/TypeScript conformance suite for C4 artifact and link
   parsing. The same concepts exist in Rust and plugin code, with partial
   fixture sharing today; this would reduce drift as `.c4` support grows.
2. Define a short vault trust model. The repo now spans local config, source
   indexing, generated state, Obsidian SVG rendering, external editor URLs, and
   optional native tools; a written boundary would make future security reviews
   less ad hoc.

## Not Audited

This was a standard audit, not a deep whole-repo audit. It did not include
manual Obsidian/browser UI testing or Rust advisory scanning. Previously
resolved audit findings were not re-reported, including path confinement, JSON
serialization, DOT SVG sanitization, source-index startup scope, and release
Rust pinning.

## Considered And Rejected

- DOT SVG insertion was not re-reported because the current code sanitizes DOT
  SVG and the plugin core test covers removal of scripts, event handlers,
  dangerous links, and Graphviz javascript URLs.
- Mermaid SVG insertion was not reported because the renderer initializes
  Mermaid with `securityLevel: "strict"` before inserting generated SVG.
- `git show` usage in `query diff` and enforcement reads passes arguments
  directly to `Command`, not through shell-built strings, so this audit did not
  find command injection there.

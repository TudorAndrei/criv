# criv Improvement Audit Issues

Written against commit: `05b1bd4`

Scope: standard `$improve` audit across correctness, security, performance,
tests, architecture, dependencies, DX/tooling, docs, and direction. This file
records vetted findings only; no implementation changes were made.

## Verification Baseline

Commands run:

```sh
cargo test --workspace
npm --prefix .obsidian/plugins/criv test
npm --prefix .obsidian/plugins/criv run format:check
npm --prefix .obsidian/plugins/criv run lint
cargo run --quiet -- check
cargo run --quiet -- enforce --stage ci
npm audit --prefix .obsidian/plugins/criv --audit-level=high
cargo audit --version
```

Observed results:

- Rust tests passed: 49 library tests, 3 CLI workflow tests, 2 WASM helper tests,
  plus doc test harnesses.
- Obsidian plugin test, format check, and lint passed.
- `criv check` passed.
- `criv enforce --stage ci` passed, but printed
  `ESLint: skipped 7 file(s); tool not found`.
- `npm audit` reported one high advisory for `esbuild@0.25.5`.
- `cargo audit` is not installed in this environment.

Resolved since this audit:

- The maintained Obsidian plugin package and generated `criv init` template now
  pin `esbuild@0.28.1`; the current high-severity plugin npm audit reports 0
  vulnerabilities.

Not run:

- `mise run plugin-build`, because it rebuilds generated plugin/WASM artifacts.
- `criv watch --once`, because it rewrites `.criv/` state.

## Findings

| # | Finding | Category | Impact | Effort | Risk | Confidence | Evidence |
|---|---|---|---|---|---|---|---|
| 1 | Add normal CI for PR/push checks | DX / Tests | The only workflow is tag-triggered release, so `hk check`, tests, plugin checks, and criv validation rely on local hooks until release time. | M | LOW | HIGH | `.github/workflows/release.yml:3`, `hk.pkl:112` |
| 2 | Exclude generated plugin bundle from criv source graph | Performance / Correctness | `query nodes --kind code --without-docs` took about 54 seconds and emitted many meaningless symbols from `.obsidian/plugins/criv/main.js`; graph/search results are noisy. | S | LOW | HIGH | `criv.toml:5`, `.obsidian/plugins/criv/package.json:5`, `src/source_graph.rs:93` |
| 3 | Make `search --files` honor `--paths` and `--lang` filters | Correctness | `criv search --files main --paths 'src/**'` still returned plugin, scripts, config, and crate files. | S | LOW | HIGH | `src/search.rs:113`, `src/search.rs:192` |
| 4 | Validate source reference fragments, not just file paths | Correctness / Tests | Docs can target `src/lib.rs#missing_symbol` and pass as long as `src/lib.rs` exists, despite README promising symbol/source target validation. | M | MED | HIGH | `README.md:31`, `src/check.rs:422`, `src/state.rs:191` |
| 5 | Align `criv enforce` JS/TS tooling with repo plugin tooling | DX / Tooling | `criv enforce --stage ci` tries ESLint, while this repo uses `oxlint` through npm/mise, so native JS/TS enforcement is inert here. | S/M | LOW | HIGH | `src/enforce.rs:487`, `.obsidian/plugins/criv/package.json:12`, `hk.pkl:49` |
| 6 | Resolve high `esbuild` npm audit advisory | Dependencies / Security | Resolved after bumping the maintained plugin package and generated `criv init` template to `esbuild@0.28.1`; current npm audit reports 0 vulnerabilities. | S/M | MED | HIGH | `.obsidian/plugins/criv/package.json:25`, `src/init/templates.rs:333`, `npm audit --prefix .obsidian/plugins/criv --audit-level=high` |
| 7 | Fix frontmatter diagnostic line offsets | Correctness / DX | Pattern reference diagnostics can point at frontmatter-relative lines instead of file lines, slowing remediation. | S | LOW | MED | `src/vault.rs:546`, `src/check.rs:435` |

## Recommended Plan Set

Default plan candidates:

1. Add normal CI for PR/push checks.
2. Exclude generated plugin bundle from criv source graph.
3. Make `search --files` honor path/language filters.
4. Validate source reference fragments.

Dependency order:

- Add CI first so subsequent behavior changes have a reliable remote baseline.
- Exclude generated source graph noise before deeper graph/query work.
- Add characterization tests before changing source fragment validation semantics.

## Direction Options

1. Add a first-class `criv ci` command that runs the non-mutating verification
   path. The README and release docs list several commands, while hk owns the
   real check graph; a CLI-level command would make generated hooks, CI, and
   external users less dependent on knowing this repo's hk setup. Trade-off:
   avoid duplicating hk-specific plugin build behavior too tightly.

2. Extend the Obsidian panel from linked sources in the active note toward
   coverage/drift triage. The state already contains source index and graph
   data, and `query coverage --by module` shows whole plugin/WASM areas
   ungoverned; surfacing ungoverned files in the panel would fit criv's purpose.
   Trade-off: plugin UI is already a large file and should be split first.

## Considered But Not Planned Yet

- Full Rust dependency audit: blocked by missing `cargo audit` in the current
  environment.
- Full generated plugin build verification: skipped because it rewrites
  generated artifacts.
- Exhaustive review of every source line: this was a standard-depth,
  hotspot-weighted audit.

# criv Audit Issues

Audit date: 2026-07-25
Audited commit: `d549a2b`

This file records vetted findings from a read-only improvement audit. It is an
issue index, not an implementation plan. Convert a finding into a focused plan
before editing production code.

The previous audit (2026-06-21, commit `56e1bce`) is in git history. All of its
issues are resolved: issues 1, 2, and 6 were fixed directly, issue 3 became
plan 003, issue 5 became plan 012, and issue 4 was settled as a documented
decision in `docs/dependency-evaluations.md`. See `plans/README.md`.

## Verification Baseline

The audit verified the baseline with these read-only commands:

- `npm audit --prefix extensions/vscode-criv --audit-level=high` reported one
  high advisory (see issue 9).
- `npm audit --prefix .obsidian/plugins/criv --audit-level=high` reported
  `found 0 vulnerabilities`.
- `cargo audit --no-fetch` matched the recorded snapshot in
  `docs/dependency-evaluations.md` byte-for-byte, so it is settled, not
  re-reported.

Issues 1, 2, and 3 were reproduced end-to-end against `target/debug/criv`. All
other findings were confirmed by reading the cited lines.

## Planning Order

- Issue 17 should land before issue 3. The `check --fix` characterization test
  is what makes the fix verifiable.
- Issue 1 should land before issue 7. Both touch glob matching at the same call
  sites; fixing the empty-set semantics first avoids redoing the matcher hoist.
- Issue 6 should land before issue 9. Once oxlint actually resolves it may
  surface real findings; landing an audit gate at the same time would deliver
  two unrelated failures together.
- Issue 12 should land before the CI-annotation direction item, because a clap
  subcommand set regenerates `docs/query-reference.md` from `--usage`.
- Issues 2, 4, 5, 8, 10, and 11 are independent of everything else.

## Issue 1: Match All Source Files When No Path Filter Is Given

Category: Correctness
Effort: S
Fix risk: MED
Confidence: HIGH
Status: Open

A `globset::GlobSet` built from zero globs matches nothing, so an empty `paths`
slice skips every source file instead of scanning all of them.

Evidence:

- `src/structural.rs:76` builds `GlobMatcher::new(paths)` and
  `src/structural.rs:78` skips any file it does not match.
- `src/search.rs:117-122` leaves `paths` empty when the user passes neither
  `--paths` nor `--lang`, then hands it to `structural::find` at
  `src/search.rs:127`.
- `src/state.rs:564` calls `structural::find_pattern_id(root, vault,
  pattern_id, &[])` on the full-rebuild path for every `[patterns.*]` in
  `criv.toml`.
- The grep path treats empty as "all" instead: `src/source_index.rs:482` uses
  `patterns.is_empty() || ...`.
- Reproduced: `criv search 'fn $NAME() { $$$ }'` returns zero rows, while
  `criv search 'fn $NAME() { $$$ }' --paths 'src/**'` returns many.

Impact:

`criv search '<ast-grep pattern>'` and `criv search --pattern-id <id>` silently
report zero matches unless the user also passes `--paths` or `--lang`. This is a
false negative on one of the tool's core surfaces, and it is indistinguishable
from a genuine no-match result. Separately, patterns declared in `criv.toml` are
written to `.criv/state.json` as permanently empty arrays, so the Obsidian and
VS Code pattern-coverage views are wrong for every configured pattern.

Fix sketch:

Make an empty pattern list mean "match everything" in `structural::find`, by
holding an `Option<GlobMatcher>` that is `None` when `paths.is_empty()`, mirroring
`path_allowed` in `src/source_index.rs`. Note that `src/state.rs:533` currently
relies on the opposite behavior: when incremental `scoped_changed_paths` filters
every changed file out of scope it passes `&[]` and expects zero matches. That
call site must return early with an empty result instead of relying on the
empty-set semantics, or the fix turns an incremental no-op into a full rescan.

Verification:

- Add a `src/structural.rs` test asserting an empty `paths` slice scans all
  source files.
- Add a `src/state.rs` test asserting that an incremental rebuild whose changed
  files are all out of scope still produces zero matches.
- Add a `tests/cli_workflows.rs` regression for `criv search '<pattern>'` with
  no path or language flag.
- Run `cargo test --workspace`.

## Issue 2: Report File-Relative Line Numbers For Note Bodies

Category: Correctness
Effort: M
Fix risk: LOW-MED
Confidence: HIGH
Status: Open

Wiki-link, heading, and Mermaid C4 line numbers are computed over the note body
after frontmatter is stripped, and the frontmatter offset is never added back.

Evidence:

- `src/vault.rs:501-521` — `split_frontmatter` returns `body = contents[next..]`,
  the file contents with the frontmatter block removed.
- `src/vault.rs:484`, `src/vault.rs:492`, and `src/vault.rs:496` compute 1-based
  line numbers over `note.body` for wiki links, headings, and C4 diagrams.
- Consumers treat those as file lines: `src/check.rs:1054`, `src/check.rs:1069`,
  `src/check.rs:1079`, `src/check.rs:1097`, the C4 diagnostics at
  `src/check.rs:880-979`, and the heading node ID at `src/state.rs:243`.
- Reproduced against committed state: `.criv/state.json` records
  `docs/adr/0001-local-cli-vault-architecture.md#L2:H1`, but that heading is on
  line 14 of the file. The offset is exactly the 12-line frontmatter block.
- Standalone `.c4` artifacts are unaffected, because `src/c4_artifact.rs:205`
  passes `start_line = 0` over the whole file.

Impact:

Frontmatter is required on every vault note, so every diagnostic anchored to a
note body points at the wrong line — usually into the frontmatter block itself.
This degrades GitHub annotations (`src/check.rs:1273-1279`), the VS Code
diagnostic collection (`extensions/vscode-criv/src/checkDiagnostics.ts:46`), and
Obsidian jump-to-line. Heading node paths in `.criv/state.json` are wrong for
every consumer that resolves them.

Fix sketch:

Have `split_frontmatter` also return the number of lines it consumed, including
both delimiters, store that on `Note`, and add it once to the `line` values
produced at `src/vault.rs:484-496`. Apply the offset in exactly one place. Review
existing tests that assert body-relative numbers; `src/c4.rs:494-497` asserts
against a frontmatter-less fixture and stays valid.

Verification:

- Add a `src/vault.rs` test covering a note with and a note without frontmatter.
- Add a `src/check.rs` regression asserting a `broken-link` diagnostic line equals
  the real file line in a note with frontmatter.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- watch --once` and confirm heading node IDs in
  `.criv/state.json` match real file lines.

## Issue 3: Stop `check --fix` From Aborting On Markdown Outside The Docs Tree

Category: Correctness
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The markdown lint pass walks the whole repository root, but the fix pass can only
write inside the configured vault docs directory, so any fixable Markdown file
outside `docs/` aborts the command.

Evidence:

- `src/check.rs:120` calls `markdown_files(root, &config)`, and
  `src/check.rs:193-227` walks the entire repository root. `.rumdl.toml` excludes
  only `.criv/**`, `.obsidian/**`, `plans/**`, and `target/**`, so `README.md`,
  `AGENTS.md`, `PLAN.md`, `TODO.md`, and `ISSUES.md` are all linted.
- `src/check.rs:250-262` publishes through `write_atomic_in(write_scope.root,
  write_scope.docs_dir, destination, contents)`, where `docs_dir` is
  `vault_config.docs_dir`.
- `src/util.rs:159-165` refuses any destination outside the allowed directory.
- `src/check.rs:262` propagates that error with `?`, so the whole command aborts
  before printing any diagnostics.
- `hk.pkl:12-17` gives the `staged_docs` step the glob `**/*.md` and the fix
  command `cargo run --quiet -- check --fix`, so this repository's own
  `mise run fix` and pre-commit fix path are exposed.
- Reproduced in a temporary vault with a fixable root-level `README.md`:
  `criv: refusing to write README.md outside allowed vault directory docs`,
  with zero diagnostics printed.

Impact:

`criv check --fix` fails hard, prints nothing useful, and surfaces a path-security
message for what is actually a scope-policy decision. The failure is
order-dependent because the file list is sorted, so `AGENTS.md` aborts the run
before `docs/` is ever reached.

Fix sketch:

Settled in the 2026-07-25 criv-me session and recorded in ADR-0044: `check --fix`
rewrites every Markdown file it lints inside the repository root. Pass
`Path::new(".")` as the allowed write directory for destinations outside the docs
tree. That retains root confinement, symlink rejection, and relative-path
validation — `prepare_confined_write` applies all three at every allowed
directory — and drops only the docs-subdirectory narrowing. Scope control belongs
to the `.rumdl.toml` exclude list. The alternative of linting files `--fix` will
never rewrite was rejected under ADR-0021, which requires user-facing flags to
correspond to active behavior.

Verification:

- Add the CLI coverage described in issue 17 first.
- Add a `tests/cli_workflows.rs` case with a fixable root-level `README.md`
  asserting the chosen behavior and a zero exit status.
- Run `cargo test --workspace`.
- Run `mise run fix` in a checkout with a deliberately unformatted root-level
  Markdown file.

## Issue 4: Reuse The Persisted Source Graph In `watch --once`

Category: Performance
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The `--once` path passes `previous_graph = None`, so the fingerprint-reuse branch
can never hit and every indexed file is re-parsed.

Evidence:

- `src/watch.rs:28` calls `rebuild(root, None, None)` for the `--once` branch.
- `src/watch.rs:152-157` threads that `None` into
  `Vault::load_incremental_with_source_index(root, None, ...)`.
- `src/vault.rs:194` then calls `SourceGraph::build_incremental(root,
  &source_files, None)`, and `src/source_graph.rs:262-272` cannot reuse anything
  with `previous = None`, so `parse_source_file` runs for every file.
- `src/vault.rs:128-131` shows the correct pattern: `Vault::load`, used by
  `check`, `enforce`, `query`, and `search`, starts from
  `crate::source_graph::load_cached(root)`.

Impact:

`criv watch --once` is the first command in the generated pre-commit hook
(`src/init/templates.rs:67-83`), so every commit pays a full tree-sitter and
ast-grep parse of every indexed file even when one file changed. The cache
written at `src/vault.rs:195` is then read by the two commands that follow, so
the optimization exists and only the hot-path caller bypasses it. The
`watch_once_warm` case in `scripts/measure-performance.sh` is cold by
construction, which is why this was not visible in the existing benchmark.

Fix sketch:

Load the cached graph before the `--once` rebuild and pass it as
`previous_graph`. Reuse is already keyed on a blake3 content fingerprint
(`src/source_graph.rs:260-266`), the same guard `Vault::load` depends on, so this
does not weaken invalidation. The long-running startup rebuild can take the same
treatment.

Verification:

- Add a `tests/cli_workflows.rs` test asserting a second consecutive
  `criv watch --once` reports zero changed files.
- Run `cargo test --workspace`.
- Run `mise run perf` before and after and record the `watch_once_warm` delta.

## Issue 5: Detect And Reclaim A Stale Watch Lock

Category: Correctness
Effort: S-M
Fix risk: LOW
Confidence: HIGH
Status: Open

The watch lock is released only in `Drop`, with no owner recorded and no
staleness check, so an interrupted watcher blocks every later commit.

Evidence:

- `src/watch.rs:210-227` — `WatchLock::acquire` uses `create_new_in` with no PID,
  timestamp, or liveness check, and fails unconditionally on `AlreadyExists`.
- `src/watch.rs:232-236` — the file is removed only in `Drop`, which does not run
  on `SIGINT`, `SIGTERM`, or `SIGKILL`, and does not run on panic either because
  `Cargo.toml:19` sets `panic = "abort"` for the release profile.
- `src/watch.rs:27` acquires the lock before the `options.once` branch at
  `src/watch.rs:28`, so `criv watch --once` is blocked by the same orphaned file.
- `src/init/templates.rs:67-83` makes `"$CRIV_BIN" watch --once` the first command
  in the generated pre-commit hook, under `set -eu`.

Impact:

Ctrl-C on a long-running `criv watch`, which is the normal way to stop it, leaves
`.criv/watch.lock` behind. Every subsequent `git commit` then fails at the first
hook line, and the VS Code "criv: run watch once" command fails the same way.
Recovery requires knowing to remove the lock file, and the error message does not
say so. This is a distinct gap from the previously fixed `watch --once`
serialization: that change is what turned an orphaned lock into a
commit-blocking one.

Fix sketch:

Write the owning PID and start time into the lock file. On `AlreadyExists`,
re-read it and reclaim the lock when the recorded process is not alive, otherwise
fail as today. Extend the message at `src/watch.rs:216-219` to name the recovery
step. Optionally install a signal handler that removes the lock on `SIGINT` and
`SIGTERM`.

Verification:

- Add a `src/watch.rs` test that pre-creates a lock owned by a dead PID and
  asserts `watch --once` reclaims it.
- Add a test asserting a lock owned by a live process is still rejected.
- Run `cargo test --workspace`.

## Issue 6: Resolve Oxlint The Way ADR-0024 Specifies

Category: Tech Debt / Enforcement
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

`criv enforce` looks up JavaScript and TypeScript lint tools by bare name only, so
a package-local oxlint is never found and enforcement silently reports success.

Evidence:

- `src/enforce.rs:852` — `tool_on_path` returns `ToolCommand::Name(name)`
  unconditionally.
- `src/enforce.rs:803-804` spawn bare `oxlint` and `ruff`.
- `src/enforce.rs:856-867` — `ToolCommand` is now a one-variant enum with a
  `program()` match, a leftover from the removed lookup.
- `src/enforce.rs:836-839` prints `"Oxlint: skipped N file(s); tool not found"`
  and returns `Ok(0)`.
- `docs/adr/0024-oxlint-only-javascript-typescript-enforcement.md:35-37` decides
  oxlint should be found "from the repository root, the Obsidian plugin package,
  or `PATH`".
- oxlint is only a devDependency
  (`.obsidian/plugins/criv/package.json:27`,
  `extensions/vscode-criv/package.json:165`) and is absent from `mise.toml`, so
  it is not on `PATH` in this repository.

Impact:

Every `criv enforce --stage commit|push|ci` in this repository, and in any vault
that installs oxlint locally rather than globally, skips JavaScript and
TypeScript linting while reporting success. The only real lint gate is hk's
`plugin-lint` and `vscode-lint` steps; `enforce`'s native-tool path enforces
nothing. This is decision drift: the code no longer does what the accepted ADR
says.

Fix sketch:

Reinstate package-local `.bin` probing via a `ToolCommand::Path` variant,
covering the repository root, the Obsidian plugin package, and
`extensions/vscode-criv`, which post-dates the ADR. Keep the `PATH` fallback. If
the intended decision is now `PATH`-only, ADR-0012 makes accepted ADRs immutable,
so that requires a superseding ADR rather than an edit to ADR-0024.

Verification:

- Add a `src/enforce.rs` test asserting resolution prefers a package-local
  binary over `PATH`.
- Add a test asserting the skip message still appears when no binary exists
  anywhere.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- enforce --stage ci` and confirm oxlint actually runs.

## Issue 7: Compile Each Glob Once Instead Of Once Per File

Category: Performance
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

`glob_matches` builds a fresh `GlobSet` on every call, and it is called inside
filters over the full source-file list.

Evidence:

- `src/util.rs:432-436` — `glob_matches` constructs a one-element
  `GlobMatcher`, and therefore a glob-to-automaton compile, per call.
- `src/vault.rs:348-352` — `source_files_matching_glob` calls it inside a
  `.filter()` over every source file, so one compile per source file.
- `src/state.rs:256-258` runs that for every note and every governs entry;
  `src/vault.rs:381-387` defaults decision notes to `"**"`, so with 44 ADRs and
  roughly 100 indexed files this is about 4,400 `GlobSet` builds per state build.
- The same shape appears at `src/check.rs:394`, `src/enforce.rs:241`,
  `src/state.rs:594-597`, and `src/state.rs:610-613`.
- The correct pattern already exists in this codebase: `FffSourceIndex` holds a
  prebuilt `GlobMatcher` at `src/source_index.rs:143`.

Impact:

Every state build pays glob compilation proportional to notes times globs times
files, rather than compilation proportional to globs plus matching proportional
to files. State building runs on every commit through `watch --once`, and the
same shape runs again in `check` and `enforce` within the same hook invocation.

Fix sketch:

Add a `Vault::source_files_matching_globs(&[String])` backed by one compiled
`GlobMatcher` and switch the call sites in `state.rs`, `check.rs`, and
`enforce.rs` to it. Keep `glob_matches` for genuine one-shot callers. Preserve
the `matches.is_empty()` fallback ordering at `src/vault.rs:355-362`.

Verification:

- Add a `src/vault.rs` test asserting multi-glob matching returns the same set
  and ordering as repeated single-glob matching, including the empty-match
  fallback.
- Run `cargo test --workspace`.
- Run `mise run perf` before and after.

## Issue 8: Re-Sync `.claude/skills` With `assets/skills`

Category: DX
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The third copy of the agent runtime skills has drifted and sits outside criv's
own configured source roots, so criv cannot see the drift.

Evidence:

- `diff -rq assets/skills .claude/skills` reports four of six `SKILL.md` files
  differ. `.agents/skills` is byte-identical to `assets/skills`.
- `.claude/skills/criv-me/SKILL.md:31` still teaches the non-portable link form
  `[[ADR-0007]]`, which `criv check` flags as `non-portable-note-link` under
  ADR-0020. `assets/skills/criv-me/SKILL.md:31-33` carries the corrected portable
  form plus the inline `policy.patterns` guidance from the ADR-0039, ADR-0040,
  and ADR-0041 work.
- `criv.toml:22-24` lists `assets/skills` and `.agents/skills` as source roots
  but not `.claude/skills`.

Impact:

Agents doing work in this repository — the primary development mode per
`AGENTS.md` — load the stale `.claude/skills` copy and are instructed to write
link forms and policy layouts that the current `criv check` and `criv enforce`
reject. That is self-inflicted rework, and the drift-detection tool is blind to
its own third copy.

Fix sketch:

Re-copy `assets/skills/*` into `.claude/skills/`, add `.claude/skills` to the
`criv.toml` `[source] roots` list, and add a check-gate step or shared test
asserting the three trees are byte-identical so the drift cannot recur.

Verification:

- `diff -rq assets/skills .claude/skills` reports no differences.
- `diff -rq assets/skills .agents/skills` reports no differences.
- Run `cargo run --quiet -- check`.
- Run `mise run check`.

## Issue 9: Wire Dependency Auditing Into The Check Gate

Category: Dependencies
Effort: S
Fix risk: LOW-MED
Confidence: HIGH
Status: Open

`cargo-audit` is pinned as a project tool but never invoked, and the two npm
packages are never audited at all.

Evidence:

- `mise.toml:11` pins `"cargo:cargo-audit" = "0.22.2"`.
- `mise.toml:22-24` — `[tasks.check]` runs only `hk check --all`; no task in the
  file invokes `cargo audit` or `npm audit`. `.github/workflows/ci.yml:73-74`
  runs exactly that task.
- `.github/` contains only `workflows/`, so there is no `dependabot.yml` and no
  Renovate configuration providing an advisory feed either.
- Observed: `npm audit --prefix extensions/vscode-criv --audit-level=high`
  reports `brace-expansion <=5.0.7` (GHSA-mh99-v99m-4gvg, high) reaching the
  project as `@vscode/vsce@3.9.2 -> minimatch@10.2.5 -> brace-expansion@5.0.6`.
  That is the VSIX packaging path (`mise.toml:74-76`), not extension runtime
  code, so direct impact is limited.
- `.obsidian/plugins/criv` reports `found 0 vulnerabilities`.

Impact:

The project has deliberately built an advisory posture — the recorded snapshot in
`docs/dependency-evaluations.md` and the zizmor gate at `mise.toml:6` — but the
enforcement loop is missing for dependencies. Rust advisories are re-discovered
only when someone reruns `cargo audit` by hand, and the npm trees are never
checked. The `brace-expansion` advisory is the proof the gap is real rather than
theoretical.

Fix sketch:

Add a `[tasks.audit]` running `cargo audit --no-fetch` non-blocking, respecting
the explicit decision in `docs/dependency-evaluations.md:60` not to add a failing
Rust gate yet, plus blocking `npm audit --audit-level=high` for both packages,
which have a hosted advisory source and are reproducible via `npm ci`. Call the
task from CI. Separately run `npm audit fix` in `extensions/vscode-criv` and
commit the lockfile change.

Verification:

- `npm audit --prefix extensions/vscode-criv --audit-level=high` reports zero
  high advisories.
- `mise run audit` exits zero on a clean tree.
- Run `mise run check`.

## Issue 10: Confine `criv init` Writes The Way Generated Writes Are Confined

Category: Security
Effort: S-M
Fix risk: LOW
Confidence: HIGH
Status: Open

`criv init` is the only remaining write path that does not go through the
symlink-rejecting confinement helper the rest of the codebase uses.

Evidence:

- `src/util.rs:26-35` — `write_new` tests `path.exists()`, which resolves
  symlinks and returns false for a dangling one, then calls `fs::write`, which
  follows the link and writes to its target.
- `src/util.rs:37-54` — `append_line_if_missing`, used for `.gitignore` at
  `src/init.rs:79`, reads and rewrites through a symlink the same way.
- `src/init.rs:340-358` — every template destination goes through
  `write_new(&root.join(path), ...)`, unconfined. That covers `criv.toml`,
  `.criv/state.json`, `docs/adr/README.md`, and the skills and plugin source
  sets.
- `src/init.rs:247-262` — `write_hook` repeats the pattern for
  `.githooks/pre-commit` and `.githooks/pre-push`, and `src/init.rs:264-272`
  then chmods the resolved target to `0o755`.
- Contrast `src/util.rs:156-206`, whose doc comment states the intended
  confinement contract, and the generated-write call sites that already use it:
  `src/state.rs:418`, `src/source_graph.rs:165`, `src/check.rs:257`,
  `src/architecture.rs:23`.

Impact:

`criv init` is the first command a user runs against a repository they just
cloned, and git tracks symlinks, so a hostile or merely misconfigured repository
can carry a symlink at any of roughly forty fixed template destinations. Content
then lands outside the vault root, and for the two hook paths a file is written
and marked executable. The 2026-07-23 hardening covered the check and watch
generated-write paths but did not migrate init, so the tool's first filesystem
contact with untrusted repository content is the unconfined one.

As of 2026-07-25 this is recorded non-compliance rather than an audit opinion:
ADR-0044 governs `src/init.rs` and states in its Consequences that `criv init`
does not yet meet the confinement rule.

Fix sketch:

Add a confined `write_new_in(root, allowed_dir, destination, ...)` built on
`prepare_confined_write` and `create_new_in`, and switch `write_template`,
`write_hook` with allowed directory `.githooks`, and the `.gitignore` append to
it. Pre-canonicalize `root` in `init::run` the way `install_git_hooks` already
does at `src/init.rs:169-170`. Reject rather than silently skip when a
destination component is a symlink, and surface that as an init error.

Verification:

- Add a Unix-only `src/init/tests.rs` case mirroring `src/util.rs:572-591`,
  asserting init errors rather than writing when a template destination is a
  symlink.
- Add a case asserting the hook paths behave the same.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- init` in a scratch repository and confirm unchanged
  behavior on a clean tree.

## Issue 11: Add A Bootstrap Task So `mise run check` Works On A Fresh Clone

Category: DX
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

The documented single verification command depends on npm dependencies that no
task installs.

Evidence:

- `AGENTS.md:12-14` presents `mise run check` as the command to "run before
  finishing any change; it is what CI runs".
- `hk.pkl:69-73`, `hk.pkl:96-100`, and `hk.pkl:105-112` make the `check` hook run
  `plugin-test`, `plugin-build`, `vscode-test`, and `vscode-json-diagnostics`,
  all of which shell into `npm --prefix ...`.
- `mise.toml` has no npm install task; its only hook is
  `postinstall = "hk install --mise"` at `mise.toml:17`.
- `.github/workflows/ci.yml:69-73` compensates with two explicit `npm ci` steps
  before `mise run check`.

Impact:

A new contributor or agent following `AGENTS.md` on a clean checkout gets a
mid-gate failure inside a bundler or test runner rather than a missing-dependency
message, and has to reverse-engineer the two `npm ci` invocations from CI or from
two separate README sections. CI's dependency on those steps is invisible from
the task definitions.

Fix sketch:

Add a `[tasks.bootstrap]` running both `npm --prefix ... ci` invocations, make the
npm-backed tasks depend on it or run it from the mise postinstall hook, and
reference it from the `AGENTS.md` verification table.

Verification:

- In a fresh clone with no `node_modules`, `mise install && mise run check`
  succeeds.
- Run `mise run check` in the existing checkout and confirm no regression.

## Issue 12: Make `criv query` A Real Clap Subcommand Set

Category: Tech Debt
Effort: M
Fix risk: MED
Confidence: HIGH
Status: Open

Query names are dispatched from a string match, with the valid set duplicated
across three hand-maintained places.

Evidence:

- `src/query.rs:24-33` takes `name: String` plus an untyped `values: Vec<String>`.
- `src/query.rs:37-101` dispatches on `options.name.as_str()` and falls through
  to `"query \`{other}\` is not implemented in this MVP"` at
  `src/query.rs:97-100`.
- The valid names live only in the `after_help` string at `src/lib.rs:57` and in
  `docs/query-reference.md:23-40`.
- Per-query flags such as `--by`, `--kind`, and `--without-docs` are accepted for
  every query name, valid or not.

Impact:

Three places must be edited in lockstep for every new query, and they will drift.
The exported Usage spec required by ADR-0019, which generates shell completions,
Markdown, and manpages, cannot complete or document query names at all, so
`criv query <TAB>` produces nothing. Typos produce an error that says "MVP" in a
v0.7.0 tool and does not list the valid names, and
`criv query coverage --kind code` silently ignores the flag.

Fix sketch:

Replace `QueryOptions.name` and `values` with a `#[derive(Subcommand)]` enum whose
variants own their positional arguments and flags. Delete the `after_help` list
and regenerate `docs/query-reference.md` from `criv --usage`. Expect to update the
help-text and parse-error assertions in `tests/cli_workflows.rs` and `src/lib.rs`.

Verification:

- Run `cargo test --workspace`.
- `criv --usage | usage generate completion --file - zsh criv` lists every query
  name.
- Run `cargo run --quiet -- query coverage` and confirm unchanged output.

## Issue 13: Skip The Source Index For Docs-Only Queries

Category: Performance
Effort: M
Fix risk: LOW-MED
Confidence: MED
Status: Open

`criv query` builds the full source index and graph before dispatching, including
for subcommands that never read either.

Evidence:

- `src/query.rs:37` calls `Vault::load(root)?` before the subcommand match.
- `src/query.rs:88-96` — the `diff` arm calls `diff(root, left, right)` and never
  touches `vault`. `next-adr-id` at `src/query.rs:39`, `orphan-docs`, `cites`, and
  `cited-by` are likewise docs-only.
- `src/vault.rs:179-196` unconditionally starts the fff index, enumerates
  entries, hashes and graph-builds every source file, and writes
  `.criv/source-graph.json`.

Impact:

`criv query diff latest latest`, a snapshot-to-snapshot JSON comparison, pays the
entire source-index and graph cost. That is the `diff_latest` line in
`scripts/measure-performance.sh`. It also means a read-only query performs a
filesystem write as a side effect, so concurrent read-only queries contend on the
graph-cache write.

Fix sketch:

Dispatch `diff` before `Vault::load`, then classify the remaining subcommands as
docs-only or source-requiring and gate the index behind that. A fully lazy
`Vault` is the larger version of the same change; note that
`tests/cli_workflows.rs:80` currently relies on the `store_cached` side effect at
`src/vault.rs:195`.

Verification:

- Add a `tests/cli_workflows.rs` test asserting `criv query diff` does not create
  or modify `.criv/source-graph.json`.
- Run `cargo test --workspace`.
- Run `mise run perf` and record the `diff_latest` delta.

## Issue 14: Document The Embeddings Feature Honestly

Category: Docs / Security
Effort: S-M
Fix risk: LOW
Confidence: HIGH
Status: Open

ADR-0008 describes the semantic search backend as local, but the enabled feature
set fetches model weights and a native runtime over the network, and the flag
that triggers it ships visible on binaries that can never run it.

Evidence:

- `Cargo.toml:38-41` enables `fastembed` with `hf-hub-rustls-tls` and
  `ort-download-binaries-rustls-tls`. The first fetches model weights from the
  Hugging Face Hub at runtime; the second downloads a prebuilt ONNX Runtime
  shared library and loads it into the process.
- `src/search.rs:264-307` — `semantic_notes` creates `.criv/embeddings` and
  constructs the model with `with_show_download_progress(false)`, so the fetch is
  silent.
- `docs/adr/0008-optional-semantic-note-search.md:16-19` describes `fastembed`
  only as "the local semantic backend" and says nothing about a network fetch or
  a native-binary download.
- `docs/adr/0001-local-cli-vault-architecture.md:39` explicitly warns against
  adding network-dependent assumptions.
- `src/search.rs:64` exposes `--semantic` in `criv search --help` on every build,
  and `src/search.rs:309-313` always fails it on released binaries. The two gates
  are reported one at a time: `src/search.rs:229-232` names the config gate,
  `src/search.rs:309-313` names the build gate.
- `plans/reports/014-embeddings-spike.md` recommended documenting the real
  activation sequence, first-run model download, `.criv/embeddings` cache, and
  offline failure mode. `plans/README.md` marks plan 014 done, but no such
  documentation exists in `README.md` or `docs/`.

Impact:

This is decision drift: the ADR that governs `src/search.rs` describes behavior
the code does not have, in a tool that otherwise runs offline inside git hooks
and CI. Separately, every user of a released binary sees a `--semantic` flag that
can never work, and gets an error naming only one of the two required gates.

Fix sketch:

Add a "Semantic note search (optional)" section covering
`cargo install --features embeddings`, `index.embeddings = true`, the first-run
model cache, and offline behavior. Make the `--semantic` error name both gates at
once, or `#[cfg]`-hide the flag on non-embeddings builds. Write a superseding or
amending ADR — ADR-0012 makes accepted ADRs immutable — recording the two network
fetches and the native-library load, cross-referencing ADR-0001.

Verification:

- Run `cargo run --quiet -- search --notes x --semantic` and confirm the error
  names both gates.
- Run `cargo run --quiet -- check` after adding the ADR.
- Run `mise run check`.

## Issue 15: Keep VS Code Diagnostics Working On Large Check Output

Category: Correctness
Effort: S
Fix risk: LOW
Confidence: MED
Status: Open

The extension truncates captured command output by appending prose into the
stream, which makes the JSON unparseable.

Evidence:

- `extensions/vscode-criv/src/commandRunner.ts:3` sets
  `DEFAULT_MAX_OUTPUT_BYTES = 1024 * 1024`, and
  `extensions/vscode-criv/src/extension.ts:189-193` never overrides it.
- `extensions/vscode-criv/src/commandRunner.ts:88-91` truncates mid-chunk with
  `chunk.subarray(0, remaining)`.
- `extensions/vscode-criv/src/commandRunner.ts:98-107` then appends
  `[criv output truncated after N bytes]` to the captured bytes.
- `extensions/vscode-criv/src/extension.ts:118` feeds that string to
  `setFromJson`, which calls `JSON.parse`; `extension.ts:119-123` catches, shows
  a warning, and returns without updating the diagnostic collection.

Impact:

On a vault with enough diagnostics to exceed 1 MiB of JSON, the Problems panel
silently keeps stale results behind a parse-error warning, repeating on every
Markdown save when `criv.checkOnSave` is enabled. The mid-chunk `subarray` cut
can also split a multi-byte UTF-8 sequence, corrupting the tail even below the
cap. Confidence is MED because whether a given vault crosses the cap is
workload-dependent and was not reproduced.

Fix sketch:

Raise the cap substantially for the `check --format json` invocation, and report
truncation as a distinct boolean field on `CommandResult` rather than appending
prose into `stdout`. Have `runCheck` show a "diagnostics truncated" warning
instead of failing to parse. Guard the multi-byte cut by concatenating buffers
before decoding, or by using a `StringDecoder`.

Verification:

- Add an `extensions/vscode-criv/test/unit` case asserting `stdout` stays valid
  JSON when truncation occurs and that `truncated` is set.
- Add a case asserting a multi-byte sequence at the boundary is not corrupted.
- Run `npm --prefix extensions/vscode-criv test`.

## Issue 16: Give Snapshots A Retention Policy And An Inspection Command

Category: Tech Debt
Effort: S-M
Fix risk: LOW-MED
Confidence: HIGH
Status: Open

Content-addressed snapshots are created and read but never enumerated or removed.

Evidence:

- `src/state.rs:431-441` writes a new `.criv/snapshots/<hash>.json` and repoints
  `.criv/latest` on every rebuild. No prune, retention, or garbage-collection
  path exists in the file.
- Measured in this repository: `.criv/snapshots` holds 19 files totaling 47 MB,
  and `.criv` totals 52 MB.
- `.githooks/pre-commit` runs `criv watch --once` on every commit, so a new
  snapshot is written per distinct state.
- `docs/adr/0007-content-addressed-state-and-diffing.md:31-37` covers the diffing
  benefit and says nothing about lifecycle.
- `src/query.rs:86-93` can read snapshots by hash, but nothing lists or removes
  them.

Impact:

Every criv vault accumulates roughly 2.5 MB per distinct documentation and source
state, indefinitely, in a gitignored directory nobody inspects. On a busy
repository that reaches hundreds of megabytes within months. It is also a surface
asymmetry: `criv query diff <hash>` requires the user to list `.criv/snapshots`
by hand.

Fix sketch:

Add a retention setting such as `[state] keep = N` or an age bound, prune on
write, and add a `criv state list|prune` or `criv query snapshots` subcommand.
Pruning must never remove `.criv/latest` or a snapshot referenced by a tracked
ref, and anything more aggressive than keeping the last N should require an
explicit flag. The retention policy is a maintainer decision and probably wants a
short ADR extending ADR-0007.

Verification:

- Add a `src/state.rs` test asserting the retention bound is honored and
  `.criv/latest` survives pruning.
- Run `cargo test --workspace`.
- Run `cargo run --quiet -- watch --once` repeatedly and confirm the snapshot
  count stays bounded.

## Issue 17: Cover The Two Paths That Rewrite User Files

Category: Test Coverage
Effort: S
Fix risk: LOW
Confidence: HIGH
Status: Open

`check --fix` and incremental pattern-match reuse both mutate durable artifacts
and have no coverage.

Evidence:

- `src/check.rs:229-278` — `apply_markdown_fixes` runs rumdl's `FixCoordinator`,
  refuses out-of-root writes, publishes via `write_atomic_in`, and reports
  non-convergence after ten iterations. `grep -n '\-\-fix' tests/cli_workflows.rs`
  returns nothing, and the only related test,
  `rumdl_fixes_markdown_content_in_process` at `src/check.rs:1938`, exercises
  rumdl in-process rather than this function.
- `hk.pkl:12-17` wires `cargo run --quiet -- check --fix` into both the
  `pre-commit` and `fix` hooks, so this untested path rewrites contributors'
  files during commit. Issue 3 is what that gap allowed through.
- `src/state.rs:483-514` — `incremental_pattern_matches` carries previous matches
  forward for unchanged files, drops matches for changed files, and re-scans.
  `grep -rn "incremental" src/state.rs tests/` returns only production call sites
  at `src/state.rs:376` and `src/watch.rs:198`.
- The one integration test that drives the incremental path,
  `long_running_watch_rebuilds_docs_sources_bursts_and_recovers` at
  `tests/cli_workflows.rs:1270`, builds its vault with `init(root)`, and init's
  default config declares no `[patterns]` and no ADR `policy.patterns`, so
  `vault.patterns()` is empty and the loop at `src/state.rs:373` never executes.

Impact:

The two places where criv writes into artifacts other tools depend on — the
user's own Markdown, and `.criv/state.json`, which the Obsidian plugin, VS Code
extension, `query`, and `enforce` all consume — have no regression net. A bug in
the changed-set filter yields a state reporting a policy match in a file that no
longer contains it, with nothing to catch it. `src/state.rs` appears in 7 of the
last 40 commits.

Fix sketch:

Add a `tests/cli_workflows.rs` group for `check --fix` following the existing
`check_json_output_*` fixture style: a file with a known rumdl violation asserting
the on-disk change, an already-clean file asserting no rewrite, and the
non-convergence diagnostic if a conflicting rule pair can be configured. Add a
`src/state.rs` unit test building a vault with one ADR-local policy over two
source files, writing state, mutating one file, rebuilding incrementally, and
asserting the unchanged file's match is preserved byte-identically while the
changed file's is refreshed. Cover deletion too.

Verification:

- Run `cargo test --workspace`.
- Confirm the new `check --fix` tests fail against the current code, per issue 3.

## Lower Priority

These are confirmed but were judged not worth planning in this round:

- `docs/tooling.md:122-123` repeats the `[[0043-hawk-visibility-analysis|ADR-0043]]`
  wiki-link on two consecutive lines. It landed in commit `d549a2b`, which also
  shows the doc gate does not catch repeated wiki-links.
- `criv check --format github` and `--filter` appear in no documentation.
  `src/check.rs:96-113` applies `--filter` before the error check, so
  `criv check --filter src/` exits zero while errors exist elsewhere in the
  vault. That exit-code semantic is undocumented and can silently disable a gate.
- State summarization and selector ranking are implemented three times:
  canonically in `crates/criv-wasm/src/lib.rs:9-60`, again in
  `extensions/vscode-criv/src/wasm.ts:116-250`, and again in
  `.obsidian/plugins/criv/src/wasm.ts:33-47` plus
  `.obsidian/plugins/criv/src/core.ts:191-222`. Both bridges swallow wasm load
  failure with `.catch(() => null)`, so a missing wasm build silently degrades to
  differently-ranked results with no user-visible signal.
- `src/check.rs` is 2150 lines spanning markdown formatting, policy scanning, C4
  validation, and three output renderers, and duplicates `policy_scope_files`
  with `src/enforce.rs:238`. The recent compiled-scope-matcher fix had to land in
  both.
- `src/state.rs:372-378` re-parses each governed source file once per registered
  pattern, while the batched single-parse scanner already exists at
  `src/structural.rs:130-190` and is used by `check` and `enforce`. This
  repository declares zero patterns, so the cost lands on target vaults using the
  ADR-policy feature rather than here.
- The generated pre-commit hook runs three processes that each perform a full
  `Vault::load`, and validation runs twice within the same hook invocation
  (`src/watch.rs:171` and `src/enforce.rs:53`). Collapsing them into one
  `criv hook --stage commit` would remove roughly two thirds of the fixed
  per-invocation cost, but changes the generated hook contract and needs
  measurement first.
- `.github/workflows/ci.yml:15-18` runs one serial job. Rust compilation across
  four distinct target directories, the wasm build, and two npm installs are all
  independent lanes that could overlap.

## Not Audited

This was a standard-depth, hotspot-weighted audit. It did not cover manual
Obsidian or VS Code UI behavior, the content of the embedded agent-skill
templates beyond the drift in issue 8, GitHub Actions supply-chain posture beyond
the existing zizmor gate, or runtime profiling. Performance findings are
read-derived; issues 4, 7, and 13 each require before-and-after measurement with
`mise run perf`.

## Considered And Rejected

- Transitive crate-version duplicates such as rand, phf, and indexmap all
  originate in independent upstreams that criv does not control. Not actionable.
- The `fff-search` transitive `git2` and `bincode` advisories have a documented
  monitor-only posture in `docs/dependency-evaluations.md`. `cargo audit
  --no-fetch` matched that recorded snapshot exactly, so it was not re-reported.
- Mermaid SVG insertion in both editors is guarded by `securityLevel: "strict"`.
  Re-checked; no bypass found.
- `git show` argument handling in `query` and `enforce` passes arguments directly
  to `Command` with no shell. Re-checked; fine.
- The Obsidian DOT sanitizer `<style>` and CSS gap remains an open investigation
  rather than a finding. This audit looked specifically for evidence that
  Graphviz output can carry attacker-influenced `<style>` from `.c4` source and
  found none; the `stylesheet` graph attribute emits an `<?xml-stylesheet?>`
  processing instruction, which `.obsidian/plugins/criv/src/core.ts:262` strips.
- Path confinement on the generated-write path is complete. `src/state.rs:418`,
  `src/source_graph.rs:165`, `src/check.rs:257`, and `src/architecture.rs:23` all
  route through `write_atomic_in`, and `prepare_confined_write` re-checks for
  symlink components after `create_dir_all`, closing the TOCTOU window. Issue 10
  covers the separate init path.
- Inline ADR `policy.patterns` are compiled through ast-grep, whose `regex:`
  support is backed by the linear-time `regex` crate, so the ReDoS angle does not
  apply. Glob scoping surfaces compile errors as errors rather than matching
  everything.
- CI supply chain: both workflows set top-level `permissions: contents: read`,
  pin every action to a full SHA, and use `persist-credentials: false`. There is
  no `pull_request_target`, and the release job verifies the tag is an ancestor
  of `origin/main` before publishing. Nothing to report.
- `@types/vscode` resolving to 1.125.0 against a 1.85.0 pin is a local
  `npm install` artifact in the working tree, not committed drift. CI's `npm ci`
  installs 1.85.0.
- DOT generation escaping in `src/c4_code.rs:109-123` handles backslash, quote,
  newline, and tab and drops carriage returns, so repository-controlled symbol
  names cannot break out of the generated DOT string.

## Direction

Forward-looking options for the maintainer, not defects. Effort estimates are
coarse.

### Implement `criv install-editor`

`plans/reports/016-editor-install-spike.md` answered every design question —
separate subcommand rather than an `init` flag, published-ID default with a
`--vsix` override, explicit `--editor code|cursor`, exact failure messages, and a
fake-editor-CLI test strategy — and verified real `code` and `cursor` installs.
Nothing consumes it. `README.md:120-127` still describes it as future work and
`docs/tooling.md:103-108` tells users to run the editor CLI by hand, so the VS
Code companion has three ADRs and a test stack but no install path. Roughly 150
lines of Rust plus fake-CLI tests. Effort M. It depends on a human decision about
marketplace and Open VSX publication; shipping `--vsix` first avoids that block.

### Land CI-native annotations

`src/check.rs:1256-1294` already implements the `github` output format with unit
tests at `src/check.rs:1332-1360`, and `plans/reports/015-ci-diagnostics-spike.md`
specified the remaining steps: a CLI workflow test and a direct CI step rather
than one routed through hk, because hk prefixes stdout and breaks workflow
commands. `.github/workflows/ci.yml:73-74` never added it, so the format ships
unused. One workflow step turns roughly 45 diagnostic codes into inline PR
annotations, in the repository other criv vaults will copy. Effort S for `check`.
Extending it to `enforce` first requires promoting enforce's formatted-string
violations to the shared `Diagnostic` type, which the spike also flagged.

### Add `criv check --changed`

`src/enforce.rs:306-345` already computes an accurate, fail-closed changed set for
commit, push, and CI stages, and `src/enforce.rs:781-788` uses it to scope policy
scans. `src/check.rs:72-101` always validates the whole vault and only narrows
afterward with `--filter`, and `hk.pkl:13-18` fires the full check whenever any
Markdown file changes. Reusing enforce's machinery consolidates two paths rather
than adding a third, and extends the trajectory of the last thirty commits.
Effort M, risk MED. The real design question is which checks are safely scopable:
link resolution, duplicate-ID detection, supersession cycles, and orphan-doc
analysis are inherently global, so this should ship as an opt-in fast path with
the full check still owning the CI gate.

An LSP server was considered and rejected: it would collapse the duplicated
TypeScript diagnostics, completion, hover, and definition logic into one Rust
implementation, but it adds a long-lived server process to a project whose
ADR-0001 is deliberately one-shot-CLI-shaped, adds a JSON-RPC dependency to the
curated set ADR-0003 governs, and would not replace the webview previews. Not
worth doing. The editor duplication it would have addressed is recorded under
Lower Priority and can be handled directly.

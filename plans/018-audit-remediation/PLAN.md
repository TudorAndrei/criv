# Plan: Fix 2026-07-25 Audit Findings

## Goal

Fix the seven highest-leverage findings from the 2026-07-25 audit recorded in
`ISSUES.md` (audited at commit `d549a2b`): two silent wrong-answer bugs that make
criv report "no matches" and wrong line numbers, two bugs that block commits, one
hot-path optimization that its own caller bypasses, and one enforcement gate that
reports success while doing nothing. Every fix is small and independently
verifiable; the value is that each one currently produces a confidently wrong
result rather than an error, so none of them is visible without going looking.

This plan covers issues 17, 3, 1, 2, 4, 5, 6, and 10. It does not cover the
remaining findings in `ISSUES.md` (7, 8, 9, 11, 12, 13, 14, 15, 16), the Lower
Priority list, or the Direction items.

Issue 10 is in scope because ADR-0044, written during planning, governs
`src/init.rs`. An accepted decision that the code violates has to be fixed; there
is no version of this plan that accepts the ADR and leaves the violation.

## Approach

Phases are ordered so the codebase stays green after every commit, and so that
characterization tests land before the behavior they pin changes. `ISSUES.md`
fixes the two hard ordering constraints: issue 17 before issue 3, because the
`check --fix` test is what makes that fix verifiable; and issue 1 before any
later glob work, because it changes what an empty glob list means.

Key design decisions:

**Issue 1 — make the path scope explicit instead of overloading the empty slice.**
The bug is that `structural::find` treats an empty `paths` slice as "match
nothing", because that is what an empty `globset::GlobSet` does, while three
callers pass an empty slice meaning "match everything". Simply inverting the
behavior would break `src/state.rs:533`, which genuinely relies on empty meaning
"nothing" when incremental scoping filters every changed file out of scope.
Rather than pick one meaning and add comments explaining the other, introduce an
explicit two-variant scope type in `src/structural.rs`:

```rust
pub(crate) enum PathScope<'a> {
    All,
    Globs(&'a [String]),
}
```

`PathScope::All` skips the matcher entirely; `PathScope::Globs` keeps today's
semantics, so an empty list still matches nothing. Every caller then states its
intent at the call site and the compiler forces each one to be considered. This
is more churn than flipping a boolean, but it is the difference between a fix and
a second bug in the opposite direction.

Note that `structural::find_policies_batch` is a separate path taking
`&BTreeSet<String>` of already-resolved file paths, not globs, where empty
correctly means "no files in scope". It is not part of this change.

**Issue 2 — carry the frontmatter offset on the note, apply it once.**
`split_frontmatter` strips the frontmatter block and every line number is then
computed over the remaining body. The fix is to have `split_frontmatter` also
return how many lines it consumed and add that once where the three parsers'
results are mapped in `parse_note`. The trap: in the frontmatter-parse-error
branch at `src/vault.rs:461-478`, `body` is set to the *entire* file contents,
not the stripped body, so the offset there must be zero. Getting this wrong
double-counts on exactly the notes that are already broken.

**Issue 3 — `check --fix` fixes everything it lints.** Settled in the criv-me
session of 2026-07-25 and recorded in ADR-0044. `criv check` lints Markdown
across the whole repository but its fix pass inherited the vault docs directory
as its allowed write directory, so a fixable root-level file aborts the command.
The fix is to pass `Path::new(".")` as the allowed directory for Markdown outside
the docs tree. This is not a weakening of the guard: with `allowed_dir = "."`,
`prepare_confined_write` (`src/util.rs:156-206`) still canonicalizes the root,
still calls `reject_symlink_components` both before and after directory creation,
and still requires the resolved parent to sit under the root. Only the
docs-subdirectory narrowing is dropped. Scope control belongs to the rumdl
configuration's exclude list, which is where the operator already declared that
`plans/**` is not vault content. ADR-0021's rule that flags correspond to active
behavior rules out the alternative of linting files `--fix` will never rewrite.

**Issues 4, 6 — compliance, not new decisions.** Both are drift from decisions
already recorded. ADR-0042 states that `build_incremental` "may hydrate from or
publish `.criv` graph-cache data independently of the live fff index", so
`watch --once` passing `None` contradicts the recorded model. ADR-0024 specifies
resolving oxlint "from the repository root, the Obsidian plugin package, or
`PATH`"; the code drifted to `PATH`-only. Neither needs a superseding ADR — the
decisions stand and the code is what moved. The `extensions/vscode-criv` package
post-dates ADR-0024 and is covered by its "repository root" clause via its own
`node_modules/.bin`.

**Two ADRs were written before this plan executes**, both accepted on
2026-07-25 and validated with `criv check`:

- **ADR-0044 Vault Write Confinement** backfills the plan-017 hardening that was
  never recorded, and settles the `--fix` scope above. Governs `src/util.rs`,
  `src/check.rs`, `src/state.rs`, `src/architecture.rs`, `src/source_graph.rs`,
  and `src/init.rs`. It explicitly records that `criv init` does **not** yet
  comply (ISSUES.md issue 10), converting that finding from an audit opinion into
  measurable drift from an accepted decision.
- **ADR-0045 Note Line Identity In Generated State** decides that reported note
  positions are file-relative, that the offset is applied once at the note layer,
  and that heading node identity may change because the old identifier encoded a
  position that did not exist in the file. Governs `src/vault.rs`, `src/c4.rs`,
  `src/state.rs`, `src/check.rs`. Phase 4 implements it.

Per ADR-0012 these are immutable once accepted, so if implementation contradicts
either, stop and write a superseding ADR rather than editing them.

Out of scope: hoisting glob compilation out of per-file loops (issue 7), the
`criv init` write confinement (issue 10), and any change to the generated hook
contract in `src/init/templates.rs`. Issue 5 fixes lock reclamation but does not
add a signal handler; that is noted as deferred.

## Implementation Phases

### Phase 1: Characterize the two paths that rewrite artifacts

Tests only, no behavior change. Everything added here must pass against current
`main`; the test that captures issue 3's bug is written now, confirmed to fail,
and then committed in Phase 2 alongside its fix.

- Add a `check --fix` group to `tests/cli_workflows.rs`, following the
  `check_json_output_is_valid_for_special_characters` fixture style at
  `tests/cli_workflows.rs:542` (`TempDir` + `init(root)` + the `criv(root)`
  helper).
- Cover: a fixable Markdown file under `docs/` is rewritten on disk and reported;
  an already-clean file under `docs/` is left byte-identical.
- Add a `src/state.rs` unit test for `incremental_pattern_matches`
  (`src/state.rs:483-514`): build a vault with one ADR-local `policy.patterns`
  entry governing two source files, write state, mutate one file, rebuild
  incrementally with `changed_files` naming only that file, and assert the
  unchanged file's match is preserved byte-identically while the changed file's
  match is refreshed.
- Add a deletion case: when a governed source file is removed, its match must not
  survive into the next state.
- Locally write the root-level `README.md` fix case and confirm it fails with
  `refusing to write README.md outside allowed vault directory docs`. Do not
  commit it in this phase.
  **Commit:** `test(check): cover markdown fixes and incremental pattern reuse`

### Phase 2: Fix every Markdown file that check lints

Implements ADR-0044. Read its Decision section before starting.

- In `src/check.rs`, change `apply_markdown_fixes` (`src/check.rs:229-278`) to
  select the allowed write directory per destination: `write_scope.docs_dir` when
  the destination is inside the docs tree, `Path::new(".")` otherwise.
- Keep the `write_atomic_in` call itself unchanged. Root confinement, symlink
  rejection, and relative-path validation all still apply at `allowed_dir = "."`;
  do not bypass or reimplement `prepare_confined_write`.
- Do **not** add a warning diagnostic or a second scope concept. Files rumdl
  lints are files `--fix` rewrites; files the operator does not want touched are
  excluded in `.rumdl.toml`.
- Verify the existing `strip_prefix(write_scope.root)` guard at
  `src/check.rs:251-256` still rejects destinations outside the repository root.
- Commit the root-level `README.md` regression from Phase 1, asserting the
  command succeeds and the file is actually fixed on disk.
- Add a case asserting a file excluded in `.rumdl.toml` is neither linted nor
  rewritten, pinning the exclude list as the scope control.
  **Commit:** `fix(check): fix every markdown file that check lints`

### Phase 3: Scan all sources when no path filter is given

- Add `PathScope<'a> { All, Globs(&'a [String]) }` to `src/structural.rs` and
  change `find` (`src/structural.rs:60`) to take it. Build the `GlobMatcher` only
  for `Globs`; `All` skips path filtering entirely.
- Thread the type through `find_pattern_id` (`src/structural.rs:225`) and
  `find_policy_pattern_entry` (`src/structural.rs:236`).
- Update every caller to state its intent:
  - `src/search.rs:127` (`Mode::Structural`) — `All` when `paths` is empty, else
    `Globs`.
  - `src/search.rs:152` (`search_pattern_id`) — same.
  - `src/state.rs:564` — currently `find_pattern_id(root, vault, pattern_id, &[])`
    on the full-rebuild path; becomes `All`.
  - `src/state.rs:533`, `src/state.rs:548`, and `src/state.rs:569` — keep today's
    semantics with `Globs`, so an empty scoped list still matches nothing.
- Do **not** change `search_rule` (`src/search.rs:155-165`); it already
  substitutes resolved scope files when `paths` is empty and is not affected.
- Do **not** change `find_policies_batch`; it takes resolved file paths, not
  globs.
- Add a `src/structural.rs` test asserting `PathScope::All` scans all source files
  and `PathScope::Globs(&[])` scans none.
- Add a `src/state.rs` test asserting an incremental rebuild whose changed files
  all fall outside scope still produces zero matches.
- Add a `tests/cli_workflows.rs` regression running `criv search '<pattern>'` with
  no `--paths` and no `--lang`, asserting non-empty output.
  **Commit:** `fix(search): scan all sources when no path filter is given`

### Phase 4: Report file-relative note line numbers

- Change `split_frontmatter` (`src/vault.rs:501-521`) to also return the number of
  lines consumed by the frontmatter block, both delimiters included.
- Add the offset once in `parse_note` where the three parsers' results are mapped
  (`src/vault.rs:484` wiki links, `src/vault.rs:492` headings, `src/vault.rs:496`
  C4 diagrams).
- For C4 diagrams, pass the offset into `c4::parse_diagrams` (`src/c4.rs:120`) and
  add it to the `start_line` handed to `parse_mermaid_diagram`, so element and
  relationship lines inside the diagram are offset too.
- **Critical:** in the frontmatter-parse-error branch (`src/vault.rs:461-478`),
  `body` is the entire file contents, not the stripped body. The offset must be
  zero there. Do not apply the offset unconditionally.
- Leave `src/c4_artifact.rs:205` alone; standalone `.c4` files pass
  `start_line = 0` over the whole file and are already correct.
- Add a `src/vault.rs` test covering a note with frontmatter and a note without,
  asserting wiki-link and heading lines equal real file lines in both.
- Add a `src/check.rs` regression asserting a `broken-link` diagnostic line equals
  the real file line in a note with frontmatter.
- Review tests that assert body-relative numbers. `src/c4.rs:494-497` uses a
  frontmatter-less fixture and stays valid; update any that do not.
  **Commit:** `fix(vault): report file-relative note line numbers`

### Phase 5: Reuse the cached source graph for single watch runs

- In `src/watch.rs:27-31`, load `crate::source_graph::load_cached(root)`
  (`src/source_graph.rs:146`) before the `--once` rebuild and pass it as
  `previous_graph` instead of `None`.
- Apply the same treatment to the long-running startup rebuild.
- Reuse is already keyed on a blake3 content fingerprint
  (`src/source_graph.rs:260-266`), the same guard `Vault::load`
  (`src/vault.rs:128-131`) depends on, so invalidation is unchanged.
- Add a `tests/cli_workflows.rs` test asserting a second consecutive
  `criv watch --once` reports zero changed files.
  **Commit:** `perf(watch): reuse the cached source graph for single runs`

### Phase 6: Reclaim an abandoned watch lock

- Write the owning process ID and start time into `.criv/watch.lock` in
  `WatchLock::acquire` (`src/watch.rs:210-227`), which currently creates an empty
  file via `create_new_in`.
- On `AlreadyExists`, read the existing lock: if the recorded process is not
  alive, remove and reclaim it; otherwise fail as today.
- Treat an unreadable or malformed lock file as stale and reclaim it, so a lock
  written by an older version does not wedge the repository permanently.
- Extend the error message at `src/watch.rs:216-219` to name the recovery step.
- Add a `src/watch.rs` test pre-creating a lock owned by a dead PID and asserting
  `watch --once` reclaims it, and a test asserting a lock owned by the current
  live process is still rejected.
  **Commit:** `fix(watch): reclaim an abandoned watch lock`

### Phase 7: Resolve package-local lint tools

- Replace the one-variant `ToolCommand` enum (`src/enforce.rs:856-867`) with
  `Name(&'static str)` plus a `Path(PathBuf)` variant.
- Change `tool_on_path` (`src/enforce.rs:852`) to probe, in order,
  `<root>/node_modules/.bin/<tool>`, `.obsidian/plugins/criv/node_modules/.bin/`,
  and `extensions/vscode-criv/node_modules/.bin/`, falling back to the bare name
  for `PATH` lookup. It needs the `root` that `run_optional_tool`
  (`src/enforce.rs:808`) already has.
- Preserve the existing skip behavior and message at `src/enforce.rs:836-839` when
  no binary is found anywhere, and do not add an ESLint fallback — ADR-0024
  forbids it.
- Add a `src/enforce.rs` test asserting a package-local binary is preferred over a
  bare name, and one asserting the skip message still appears when nothing
  resolves.
  **Commit:** `fix(enforce): resolve package-local lint tools`

### Phase 8: Bring `criv init` under write confinement

Required by ADR-0044, which governs `src/init.rs`. This is not optional
follow-up: an accepted decision that the code violates must be fixed.

- Add a confined `write_new_in(root, allowed_dir, destination, contents)` to
  `src/util.rs`, built on the existing `prepare_confined_write` and
  `create_new_in`, preserving `write_new`'s "returns false if the file already
  exists" contract.
- Route `write_template` (`src/init.rs:347-358`) through it for all 16 templates.
- Route `write_hook` (`src/init.rs:247-262`) through it with allowed directory
  `.githooks`, so `set_executable` (`src/init.rs:264-272`) can no longer chmod a
  symlink target.
- Route the `.gitignore` append (`src/init.rs:79`) and the two
  `.vscode/extensions.json` writes (`src/init.rs:109`, `src/init.rs:145`) through
  confined equivalents; the latter currently use the unconfined `write_atomic`.
- Pre-canonicalize `root` in `init::run` the way `install_git_hooks` already does
  at `src/init.rs:169-170`, and review the four bare `fs::create_dir_all` calls
  (`src/init.rs:43`, `:44`, `:173`) so a symlinked ancestor cannot be created
  through.
- Reject rather than silently skip when a destination component is a symlink, and
  surface it as an init error naming the path.
- Add Unix-only tests to `src/init/tests.rs` modeled exactly on
  `confined_atomic_write_rejects_symlinked_components` (`src/util.rs:573-590`):
  one for a symlinked template destination, one for a symlinked `.githooks`
  directory. Assert the outside target is not written.
- Confirm the 13 existing init tests still pass unchanged, especially
  `init_installs_git_hooks_by_default`, `init_hooks_are_idempotent_without_force`,
  `init_force_hooks_overwrites_hooks_and_hookspath`, and the bare-repo case.
  **Commit:** `fix(init): confine scaffolding and hook writes`

## Risks & Tradeoffs

- **Phase 3 is the riskiest change.** `state_pattern_matches`
  (`src/state.rs:516-560`) has three branches with different scoping semantics,
  and `scoped_changed_paths` (`src/state.rs:587-600`) returns the scope list
  itself on a full rebuild but a filtered list incrementally. Mapping each branch
  to the wrong `PathScope` variant turns an incremental no-op into a full rescan,
  or silently reintroduces the bug. The explicit enum is chosen precisely so the
  compiler forces each site to be considered, but each mapping still has to be
  reasoned about individually rather than pattern-matched mechanically.
- **Phase 4 changes values that appear in `.criv/state.json`.** Heading node IDs
  (`src/state.rs:243`) will shift for every note with frontmatter, so the first
  rebuild after this lands produces a large state diff. That is the fix working,
  not a regression, but it will show up in `criv query diff` and in the editors.
- **Phase 7 may surface real oxlint findings on first run.** Once resolution
  works, `criv enforce` will actually lint. That is the point of the fix, but it
  can turn one green commit into a red one; land it on its own so the cause is
  unambiguous.
- **Phase 6 PID reuse.** Checking process liveness by PID alone can be fooled by
  PID reuse on a long-lived machine. Recording the start time alongside the PID
  narrows the window; it does not close it entirely. The failure mode is a
  refused lock, not a corrupted state, so this is acceptable.
- **Phase 2 widens what `check --fix` can rewrite** to any linted Markdown inside
  the repository root, including `README.md` and `AGENTS.md`. The confinement
  guards that matter are retained, and `.rumdl.toml` is the scope control, but
  this is a real behavior change: `mise run fix` will now reformat root-level
  Markdown it previously left alone. ADR-0044 records the reasoning.

## Open Questions

None remaining. All planning questions were resolved in the 2026-07-25 criv-me
session; see below.

## Resolved during planning

- Phase 4's state regeneration lands in the **same commit** as the code change.
  A split would leave one commit where `.criv/state.json` disagrees with the code
  that produced it.
- Phase 6 ships **without** a `SIGINT`/`SIGTERM` handler. PID-liveness
  reclamation fixes the observed breakage; a handler adds a dependency to remove
  a lock file that is already reclaimable. Revisit only if reclamation proves
  insufficient in practice.

- Phase 8 and symlinked vault paths: failing is intended behavior, not a
  regression. A vault's notes govern the source beside them, and ADR-0002 models
  `docs/` as the committed vault, so documentation kept outside the repository and
  symlinked in is not versioned with the code it governs. Recorded in ADR-0044's
  Decision section. This is no longer a STOP condition for Phase 8.
- Phase 2's fix scope: settled in the 2026-07-25 criv-me session in favor of
  fixing everything that gets linted. Recorded in ADR-0044. The earlier proposal
  — lint outside `docs/` but never fix, with a warning — was rejected as the
  surface-without-behavior pattern ADR-0021 argues against.
- Whether issues 4 and 6 need ADRs: no. Both are drift from ADR-0042 and ADR-0024
  respectively; the recorded decisions already describe the intended behavior.
- Whether the structural `PathScope` type needs an ADR: no. It restores ADR-0005's
  stated intent and introduces no user-visible surface beyond fixing the bug.

## Implementation notes (2026-07-27)

Recorded where execution diverged from the plan as written.

- **Phase 5** extracted a `run_once` helper in `src/watch.rs` rather than
  inlining the cache load at the call site. `SourceGraph::changed_files` is
  `#[serde(skip)]`, so the plan's "reports zero changed files" assertion is not
  observable through the CLI; the helper lets a unit test observe it directly.
  The CLI test that remains asserts the warm run produces the same snapshot as
  the cold one — reuse must not change results.
- **Phase 6** determines process liveness by invoking `ps -o lstart= -p <pid>`
  rather than adding a direct `libc` dependency for `kill(pid, 0)`. One probe
  returns both liveness and the start time used to detect PID reuse, and it
  keeps the dependency set unchanged. On non-Unix platforms liveness cannot be
  established, so a lock is treated as live and never reclaimed.
  Two existing tests wrote placeholder lock contents (`"held"`, `"active"`) that
  are now reclaimable-as-malformed; both were changed to record this live test
  process, which is what they were actually asserting.
- **Phase 8** added `util::create_dir_in` so `criv init`'s directory creation
  gets the same symlink rejection as its file writes, and deleted the now-unused
  unconfined `util::write_atomic`. `write_hook` takes the worktree root and a
  hook name instead of a full path, so the confinement root and allowed
  directory are explicit at the call site.

### Phase 5 measurement

Steady-state `criv watch --once` on this repository (108 source files, debug
binary, three consecutive runs each):

| Build | Warm `watch --once` (real) |
|-------|----------------------------|
| Before Phase 5 (`17ce52b`) | 1.55s, 1.49s |
| After Phase 5 | 1.29s, 1.30s, 1.23s |

Roughly a 15% reduction. `mise run perf` after the change reports
`watch_once_cold` 1.34s vs `watch_once_warm` 1.23s in a single pass.

### Verification not performed

- No before/after `mise run perf` snapshot was committed to `plans/reports/`;
  the numbers above were measured ad hoc against a temporary worktree.

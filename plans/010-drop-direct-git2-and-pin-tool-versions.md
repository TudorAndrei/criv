# Plan 010: Replace init's git2 usage with shell git, demote git2, and pin the floating tool versions

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/init.rs src/init/tests.rs Cargo.toml mise.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S–M
- **Risk**: MED (init's repo detection must behave identically across edge cases)
- **Depends on**: none
- **Category**: tech-debt + deps
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

The `git2` crate (libgit2 native bindings) is a direct dependency used by
exactly one code path: `criv init`'s hook installation (repo discovery, bare
check, `core.hooksPath` config). Every other git interaction in the codebase
shells out to the `git` binary (`src/enforce.rs:335,467`, `src/query.rs:493`).
Meanwhile `cargo tree --duplicates` shows TWO compiled git2/libgit2 stacks:
the direct `git2 0.21` and fff-search's transitive `git2 0.20.4`. Porting the
three init call sites to shell git removes the duplicate native stack from
the release binary (which is explicitly size-optimized, ADR-0015) and leaves
one consistent git-access pattern. Note honestly: fff-search keeps its own
git2 regardless, so this removes the *second* copy, not libgit2 entirely.

Rider: `mise.toml` pins every tool exactly except `"cargo:cargo-audit" =
"latest"` and `taplo = "latest"`, which makes `mise run check` behavior drift
over time and across machines. Pin them.

## Current state

Relevant files:

- `src/init.rs` — `use git2::{ErrorCode, Repository};` (line 10);
  `install_git_hooks` (156) calls `discover_worktree` (192) and
  `configure_hooks_path` (248); `repo.workdir()` distinguishes bare repos
  (install_git_hooks lines ~163–168).
- `src/init/tests.rs` — test setup uses `git2::Repository::init/open/init_bare`
  (lines 160, 164, 192, 208, 227, 255, 275, 283, 309) to create fixture repos
  and to READ BACK `core.hooksPath` assertions.
- `Cargo.toml:44` — `git2 = { version = "0.21", default-features = false }`.
- `mise.toml:11-12` — `"cargo:cargo-audit" = "latest"`, `taplo = "latest"`.
- Shell-git precedent to match: `src/query.rs:493-501` builds
  `Command::new("git").current_dir(root).args([...])` and maps errors to
  `CrivError`.

Excerpts of the three behaviors to reproduce (from `src/init.rs`):

```rust
fn discover_worktree(root: &Path) -> Result<Option<Repository>> {
    match Repository::discover(root) {
        Ok(repo) => Ok(Some(repo)),
        Err(err) if err.code() == ErrorCode::NotFound => Ok(None),
        Err(err) => Err(CrivError::new(format!(
            "failed to discover Git repository: {err}"
        ))),
    }
}
```

```rust
    let Some(workdir) = repo.workdir() else {
        return Ok(vec![
            "skipped Git hooks: bare repositories do not have a worktree".to_string(),
        ]);
    };
```

```rust
fn configure_hooks_path(repo: &Repository, force: bool) -> Result<String> {
    let mut config = repo.config()...;
    match config.get_string("core.hooksPath") {
        Ok(value) if value == ".githooks" => Ok("Git core.hooksPath already set to .githooks".to_string()),
        Ok(value) if !force => Ok(format!("skipped Git core.hooksPath: already set to `{value}`")),
        Ok(_) | Err(_) => { config.set_str("core.hooksPath", ".githooks")...; Ok("configured Git core.hooksPath=.githooks".to_string()) }
    }
```

Behavioral contract (must hold after the port — the messages are asserted by
tests in `src/init/tests.rs`):

1. Not inside a git repo → hooks skipped with
   "skipped Git hooks: not inside a Git repository", `init` still succeeds.
2. Bare repo → "skipped Git hooks: bare repositories do not have a worktree".
3. Worktree discovered from a SUBDIRECTORY too (`Repository::discover` walks
   up) — `criv init` in `repo/subdir` must find `repo/` and compute
   `relative_root` correctly via `repo_relative_root(workdir, root)`.
4. `core.hooksPath` read: already `.githooks` → "already set" message; set to
   something else and no `--force-hooks` → "skipped ... already set to
   `<value>`"; unset or forced → write `.githooks` to the LOCAL repo config
   (git2's `repo.config().set_str` writes `.git/config`).

Shell-git equivalents (verify each against `git` behavior in Step 1):

- Discovery + workdir: `git -C <root> rev-parse --show-toplevel` → prints the
  worktree root (exit 0); in a bare repo it fails with
  "this operation must be run in a work tree" (exit 128); outside any repo it
  fails with "not a git repository" (exit 128). To distinguish bare from
  not-a-repo: `git -C <root> rev-parse --is-bare-repository` → `true`/`false`
  (exit 0 inside any repo incl. bare; exit 128 outside).
- Config read: `git -C <root> config core.hooksPath` → value (exit 0) or
  exit 1 when unset.
- Config write (local): `git -C <root> config core.hooksPath .githooks`.

Rust toolchain/test conventions: errors via `CrivError::new(format!(...))`;
unit/integration tests for init in `src/init/tests.rs` create temp repos.
Conventional commits.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `cargo test --workspace` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Dep tree check | `cargo tree -i git2@0.21.0` | "nothing depends on" error after removal (or absent) |
| Duplicate check | `cargo tree --duplicates \| grep git2` | only 0.20.4 remains |
| Build | `cargo build --release` | exit 0 |
| Tool check | `mise install && mise run check` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `src/init.rs`
- `src/init/tests.rs`
- `Cargo.toml` (dependency table only)
- `Cargo.lock` (regenerated by cargo)
- `mise.toml` (the two `latest` pins only)

**Out of scope** (do NOT touch):
- `src/enforce.rs`, `src/query.rs` git usage — already shell-git.
- fff-search and its transitive `git2 0.20.4` — documented monitor-only
  posture (docs/dependency-evaluations.md); not this plan's problem.
- Hook file contents / templates (`src/init/templates.rs`).

## Git workflow

- Conventional commits, suggested:
  `refactor(init): use shell git for hook installation`,
  `chore(deps): drop direct git2 dependency`,
  `chore(tools): pin cargo-audit and taplo versions`.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Characterize current behavior in tests first

Read the existing init hook tests (`src/init/tests.rs:150-320`). Confirm they
cover contract points 1–4 above; add any missing case NOW while git2 still
powers the implementation — especially the subdirectory-discovery case (3)
and the "hooksPath set to something else without force" case (4). These tests
must pass before AND after the port.

**Verify**: `cargo test --workspace` → exit 0.

### Step 2: Port `discover_worktree` / bare detection / `configure_hooks_path` to shell git

In `src/init.rs`, replace the git2 calls with `std::process::Command`
invocations per the equivalents table above. Suggested shape: a small private
helper `fn git_output(root: &Path, args: &[&str]) -> Result<GitResult>`
capturing status/stdout/stderr, then:

- `discover_worktree(root) -> Result<Option<PathBuf>>` (now returns the
  worktree root instead of a `Repository`): run `--is-bare-repository`
  first — exit 128 → `Ok(None)` (not a repo; match on the status code, not
  stderr text); `true` → bare (return a marker so `install_git_hooks` prints
  the bare message); `false` → run `--show-toplevel` and return the path.
  Any other failure → `CrivError` mirroring today's "failed to discover Git
  repository: ..." message with stderr appended.
- `install_git_hooks` uses the returned worktree `PathBuf` where it used
  `repo.workdir()`; everything from `fs::canonicalize` down is unchanged.
- `configure_hooks_path(root: &Path, force: bool)` reads via
  `git config core.hooksPath` (exit 1 = unset) and writes via
  `git config core.hooksPath .githooks`, preserving the four message strings
  exactly. Run the config commands with `-C` pointing at the WORKTREE root
  (so behavior matches git2's repo-scoped config), not the criv root.
- Missing `git` binary entirely (spawn `ErrorKind::NotFound`): map to the
  skip path "skipped Git hooks: not inside a Git repository" is WRONG — that
  would lie. Return a clear `CrivError` ("failed to run git: ..."); `criv
  init` outside a repo never spawns... actually it does spawn to discover.
  Decision: spawn failure = `CrivError` propagated; a machine without git
  running `criv init` inside what would be a repo cannot install hooks and
  should hear why. Note this is a (defensible) behavior change from git2,
  which needed no binary — record it in the commit message.

Remove `use git2::...` from `src/init.rs`.

**Verify**: `cargo test --workspace` → exit 0 (including Step 1's
characterization tests, unchanged).

**Commit**: `refactor(init): use shell git for hook installation`

### Step 3: Demote git2 out of the release graph

`src/init/tests.rs` still uses git2 for fixture setup. Two options, in
preference order:

1. Port test fixtures to shell git too (`Command::new("git").args(["init"])`,
   `["init", "--bare"]`, and read-back via `git config core.hooksPath`) and
   delete the dependency entirely.
2. If (1) turns messy, move git2 to `[dev-dependencies]` — the release binary
   and `cargo tree` (non-dev) no longer include it; build-time cost for tests
   remains.

Then delete `git2` from `[dependencies]` in `Cargo.toml` and run
`cargo build` to refresh `Cargo.lock`.

**Verify**:
- `cargo tree -i git2@0.21.0` → error "package ID specification ... did not
  match any packages" (option 1) or shows only dev-scope (option 2 — use
  `cargo tree -e no-dev -i git2@0.21.0` to confirm it's out of the release
  graph)
- `cargo test --workspace` → exit 0
- `cargo build --release` → exit 0; optionally record the binary size delta
  (`ls -la target/release/criv`) in the commit body

**Commit**: `chore(deps): drop direct git2 dependency`

### Step 4: Pin the floating tools

In `mise.toml`, replace both `latest` values with the exact versions
currently resolved: get them via `mise ls | grep -E 'cargo-audit|taplo'` (or
`cargo audit --version` / `taplo --version` through `mise x`). Use those
exact versions.

**Verify**: `mise install` → exit 0; `mise run check` → exit 0.

**Commit**: `chore(tools): pin cargo-audit and taplo versions`

## Test plan

- Step 1's characterization tests are the core: not-a-repo, bare repo,
  subdirectory discovery, hooksPath already-`.githooks` /
  already-other-without-force / unset, force overwrite. All asserted on the
  exact message strings `install_git_hooks` returns today.
- Model on the existing tests in `src/init/tests.rs` (temp dirs + real git
  repos).
- Verification: `cargo test --workspace` → all pass at every step boundary.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `grep -rn 'git2' src/init.rs` → no matches
- [ ] `grep -n 'git2' Cargo.toml` → no match in `[dependencies]` (dev-deps
      acceptable per Step 3 option 2)
- [ ] `cargo tree --duplicates | grep -c 'git2'` shows only the 0.20.4 line(s)
- [ ] `grep -n 'latest' mise.toml` → no matches
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `mise run check` all exit 0
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The exact git CLI behaviors differ from the equivalents table on your
  platform (e.g. `--is-bare-repository` exit codes) — report what you
  measured; do not guess a parsing strategy around unverified output.
- Any existing init test asserts git2-specific behavior that shell git cannot
  reproduce (worktree edge cases, gitlinks/submodules).
- `mise run check`'s pinned cargo-audit/taplo versions produce different
  check results than `latest` did (e.g. an advisory appears) — that is a
  finding for the operator, not something to silence.

## Maintenance notes

- `criv init` now requires the `git` binary on PATH to install hooks (it
  already required git for the hooks to be USEFUL, so the practical change is
  nil — but the error surface moved; reviewer should confirm the not-a-repo
  and no-git-binary paths read clearly).
- If a future feature needs richer git access (blame, object reads), prefer
  extending the shell-git helpers over reintroducing libgit2 — and note
  fff-search's git2 may someday disappear upstream, completing the removal.
- Dependabot/renovate-style bumps: the two new exact pins in `mise.toml` now
  need occasional manual bumps (that's the point — deliberate upgrades).

# Plan 006: Parse each source file once per policy scan instead of once per pattern

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- src/structural.rs src/enforce.rs src/check.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none (recommended before plans/007)
- **Category**: perf
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

ADR-owned policy patterns (inline ast-grep rules in ADR frontmatter, per
ADR-0039/0040/0041) are enforced on every commit and push. The current scan
shape is: for each accepted ADR → for each of its policy patterns → call
`structural::find`, which loops over ALL in-scope source files, recompiles the
matcher per file, re-reads the file from disk, and re-parses it into a fresh
tree-sitter AST. With P patterns over F files that is P×F disk reads, P×F
parses, and P×F matcher compilations per `criv check` / `criv enforce` run.
Parsing is the expensive part and the file contents don't change mid-run. The
cost grows multiplicatively as vaults adopt more ADR policies — this is the
dominant enforcement cost.

## Current state

Relevant files:

- `src/structural.rs` — the scan engine. `find()` at line 48 (loop at 60–92),
  `compile()` at 190, `scan_source()` at 218, `find_policy_pattern_entry()`
  at 131, `find_pattern_id()` at 112. `CompiledMatcher` enum
  (Pattern | Rule) at ~line 30.
- `src/enforce.rs` — policy scan loop at lines ~130–160: iterates accepted
  ADR notes → patterns → `find_policy_pattern_entry(root, vault, pattern, &scopes)`.
  Per-pattern `scopes` come from `policy_scan_files(vault, &vault.effective_governs(note), changed_files)`.
- `src/check.rs` — `policy_violations()` at lines ~270–300: same loop shape,
  scopes from `policy_scope_files(vault, &vault.effective_governs(note))`.
- `src/search.rs` — `criv search --rule ADR-NNNN` also reaches
  `find_policy_pattern_entry` / `find_pattern_id`; single-pattern usage, so it
  gains nothing but must not regress.

Excerpt — the per-file recompile/re-parse (`src/structural.rs:60-92`):

```rust
    for source_file in vault.source_files() {
        if !path_allowed(source_file, paths) {
            continue;
        }
        if forced_language
            .is_some_and(|language| SupportLang::from_path(source_file) != Some(language))
        {
            continue;
        }
        let Some(language) = forced_language.or_else(|| SupportLang::from_path(source_file)) else {
            continue;
        };

        let matcher = match compile(source, language) {
            Ok(matcher) => matcher,
            Err(err) if forced_language.is_none() => {
                first_compile_error.get_or_insert(err);
                continue;
            }
            Err(err) => return Err(err),
        };
        compiled_any_language = true;

        let contents = read_source_to_string(root, source_file)?;
        match matcher {
            CompiledMatcher::Pattern(pattern) => {
                rows.extend(scan_source(source_file, language, &contents, &pattern));
            }
            CompiledMatcher::Rule(rule) => {
                rows.extend(scan_source(source_file, language, &contents, &rule));
            }
        }
    }
```

Excerpt — `scan_source` (`src/structural.rs:218-229`): parses a fresh AST per
call:

```rust
fn scan_source<M: Matcher>(
    source_file: &str,
    language: SupportLang,
    contents: &str,
    matcher: &M,
) -> Vec<StructuralMatch> {
    let ast = language.ast_grep(contents);
    ast.root()
        .find_all(matcher)
        .map(|matched| row_from_match(source_file, &matched))
        .collect()
}
```

Excerpt — the caller loop shape (`src/check.rs:270-300`, `src/enforce.rs:130-160`
is analogous):

```rust
    for note in &vault.notes {
        if note.status.as_deref() != Some("accepted") { continue; }
        let Some(adr_id) = &note.id else { continue; };
        let scopes = policy_scope_files(vault, &vault.effective_governs(note));
        for pattern in &note.policy_patterns {
            if !crate::structural::policy_pattern_entry_is_valid(pattern) { continue; }
            ...
            let rows = crate::structural::find_policy_pattern_entry(root, vault, pattern, &scopes)?;
```

Important semantics to preserve:

- **Per-pattern scopes.** Each ADR's patterns scan only the files matched by
  that ADR's `governs` globs (and, for `enforce` commit/push stages, only
  changed files). Different ADRs have different scopes.
- **Language filtering.** Policies always have a forced language
  (`find_policy_pattern_entry` passes `Some(language)`); files of other
  languages are skipped.
- **Error semantics.** With a forced language, a compile error is returned
  immediately. Without one (the `search --grep`-style path through `find` with
  `language: None`), compile errors are deferred and only surfaced when no
  language compiled (`compiled_any_language` logic). Do not change this.
- **Row ordering.** `find` sorts rows by (path, line, range, text) before
  returning — outputs must stay byte-identical.
- **Validation.** `find_policy_pattern_entry` calls `validate_source` before
  scanning; invalid patterns are skipped by callers via
  `policy_pattern_entry_is_valid`.

Conventions: unit tests in `#[cfg(test)] mod tests` (`src/structural.rs:302`);
conventional commits.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Tests | `cargo test --workspace` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Self-check | `cargo run --quiet -- check` | exit 0 |
| Enforce smoke | `cargo run --quiet -- enforce --stage ci` | exit 0 |
| Perf measure | `mise run perf` | completes; record before/after |

## Scope

**In scope** (the only files you should modify):
- `src/structural.rs`
- `src/enforce.rs` (only the policy-scan loop)
- `src/check.rs` (only `policy_violations`)

**Out of scope** (do NOT touch):
- `src/search.rs` — `--rule` search keeps calling the existing single-pattern
  entry points; they must keep working unchanged.
- The ADR policy parsing/validation in `src/vault.rs` / `src/config.rs`.
- tree-sitter/ast-grep dependency versions.
- Caching parsed ASTs across *processes* (that's plan 007 territory).

## Git workflow

- Conventional commit, e.g. `perf(policy): scan each source file once per run`.
- Commit in two steps if convenient (engine first, callers second) — but the
  build must pass at each commit.
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Fix the trivially hoistable compile in `find`

Inside `find`, when `forced_language` is `Some(lang)`, the matcher does not
depend on the file — hoist `compile(source, lang)` above the file loop for
that case. Keep the per-file compile only for the `None` case (language
inferred per file). This alone removes the P×F compiles for policies without
any structural change.

**Verify**: `cargo test --workspace` → exit 0.

### Step 2: Add a batch scan entry point

Add to `src/structural.rs` a batch API that takes all patterns up front and
walks files once. Suggested shape (adapt names to taste, keep them descriptive):

```rust
pub(crate) struct PolicyScanRequest<'a> {
    pub(crate) key: usize,              // caller's index to map rows back
    pub(crate) policy: &'a PolicyPattern,
    pub(crate) paths: &'a [String],     // this pattern's scope globs/files
}

/// Scans every in-scope source file once, running all matching-language
/// patterns against a single parsed AST per file. Returns rows grouped by
/// request key, each group sorted like `find` sorts.
pub(crate) fn find_policies_batch(
    root: &Path,
    vault: &Vault,
    requests: &[PolicyScanRequest<'_>],
) -> Result<BTreeMap<usize, Vec<StructuralMatch>>>
```

Implementation outline:

1. For each request: `policy_source(request.policy)` + `parse_language` +
   `compile` once. Invalid requests: return the error (callers already
   pre-filter with `policy_pattern_entry_is_valid`, so an error here means a
   bug — same behavior as today's `validate_source` inside
   `find_policy_pattern_entry`).
2. Outer loop over `vault.source_files()`. For each file, compute
   `SupportLang::from_path(file)` once; collect the subset of compiled
   requests whose language matches AND whose `paths` allow the file
   (`path_allowed(file, request.paths)`). Skip the file if none match.
3. Read the file once (`read_source_to_string`), parse once
   (`language.ast_grep(&contents)`), then for each matching request run
   `ast.root().find_all(matcher)` and push rows under the request's key.
   You will need to refactor `scan_source` so the AST is built by the caller —
   e.g. add `fn scan_ast<M: Matcher>(source_file, ast_root, matcher) -> Vec<StructuralMatch>`
   and keep `scan_source` as a thin wrapper that parses then delegates.
4. Sort each group exactly like `find` does: by `(path, line, range, text)`.

Watch out: `language.ast_grep(contents)` returns a language-specific AST type;
all requests in step 3 share the file's single language, so one parse per file
suffices (a file never parses as two languages).

**Verify**: `cargo test --workspace` → exit 0, including new unit tests (see
Test plan) proving batch output == per-pattern `find` output on the same
fixture.

### Step 3: Switch both policy loops to the batch API

In `src/check.rs::policy_violations` and the analogous loop in
`src/enforce.rs`: first collect all `(note, pattern, scopes)` triples into a
`Vec<PolicyScanRequest>` (computing each ADR's scopes exactly as today —
`policy_scope_files` in check, `policy_scan_files` in enforce), remembering
which (adr_id, pattern_id) each key maps to. Then make one
`find_policies_batch` call and rebuild the violations in the same order the
old nested loop produced them (iterate requests in insertion order, then rows
in their sorted order) so output text is unchanged.

Note: `scopes` is currently a `Vec<String>` created per note inside the loop —
you'll need to collect scopes into an owned structure that outlives the
requests (e.g. `Vec<(String, String, Vec<String>)>` first, then build requests
borrowing from it).

**Verify**:
- `cargo test --workspace` → exit 0
- `cargo run --quiet -- check` on this repo → identical output to before
  (save to files, `diff`)
- `cargo run --quiet -- enforce --stage ci` → identical output to before

### Step 4: Measure and gate

**Verify**:
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `mise run perf` → record before/after numbers in the commit message body
- `git status` clean outside in-scope files

**Commit**: `perf(policy): scan each source file once per run`

## Test plan

- In `src/structural.rs` `mod tests` (model on existing tests there):
  - `batch_matches_sequential_find`: build a small vault fixture (see how
    existing tests in that module construct vaults — reuse their helpers) with
    2+ policy patterns in the same language and overlapping scopes; assert
    `find_policies_batch` groups equal the per-pattern
    `find_policy_pattern_entry` results.
  - `batch_respects_per_pattern_scopes`: two patterns with disjoint `paths`
    scopes over the same files; assert each group only contains its scope.
  - `batch_skips_non_matching_language`: a pattern whose language matches no
    files returns an empty group (not an error).
- Existing CLI tests in `tests/cli_workflows.rs` already cover end-to-end
  enforce/check output; they must pass unchanged.
- Verification: `cargo test --workspace` → all pass.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test --workspace` exits 0, including the three new batch tests
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `cargo run --quiet -- check` and `cargo run --quiet -- enforce --stage ci`
      produce byte-identical output before vs after (diff of saved outputs)
- [ ] In `src/check.rs::policy_violations` and the enforce policy loop,
      `find_policy_pattern_entry` is no longer called inside a per-pattern loop
      (`grep -n 'find_policy_pattern_entry' src/check.rs src/enforce.rs` — only
      `src/search.rs` and `src/structural.rs` may still reference it)
- [ ] `git status` clean outside in-scope files
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts above don't match the live code (drift).
- ast-grep's API makes "parse once, run many matchers" impossible without
  cloning the AST per matcher (i.e. `find_all` consumes the root) — check the
  existing `scan_source` generics first; if a fundamental API obstacle
  appears, report with the specific type error rather than working around it
  with unsafe or excessive cloning.
- Output of check/enforce differs in any way on this repo (ordering counts).
- The borrow structure in Step 3 fights back for more than ~an hour — an
  owned `PolicyScanRequest` (String/Vec fields instead of borrows) is an
  acceptable simplification; more than that, report.

## Maintenance notes

- Plan 007 (hook pipeline consolidation) may reuse `find_policies_batch`
  across check+enforce in one process — keep the API free of enforce-specific
  assumptions.
- Reviewer: the risk is subtle semantic drift in scope filtering (per-ADR
  `governs` globs) and in the deferred-compile-error behavior for the
  non-forced-language path; the new tests target exactly those.
- Deferred: caching parsed ASTs across runs (needs invalidation design);
  parallelizing the file loop (rayon) — measure first, the single-parse fix
  may be enough.

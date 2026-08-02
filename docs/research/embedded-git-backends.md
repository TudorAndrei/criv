---
id: embedded-git-backends
kind: doc
title: Embedded Git backend comparison
---

# Embedded Git backend comparison

## Question and contract

This note compares `git2` 0.21 and `gix` 0.86 as an embedded backend for the
Git operations performed by the shipped `criv` binary. The migration boundary
is intentionally narrow: runtime behavior must remain the same while the
binary stops launching the `git` executable. Tests and repository tooling may
continue to use Git to construct fixtures.

The present contract comes from `src/enforce.rs#fn:changed_entries`,
`src/enforce.rs#fn:pre_push_changed_entries`, and
`src/query.rs#fn:load_git_state`. It includes repository discovery;
HEAD/index/worktree and tree-to-tree comparisons; rename and copy reporting;
blob reads from revisions and the index; first-parent and outgoing-commit
traversal; and the current UTF-8 and error behavior. The implementation is
currently SHA-1-specific: pre-push input accepts exactly 40 hexadecimal digits.
Adding SHA-256 support would be a separate behavior change.

## Conclusion

Prefer **`git2` 0.21 as the first implementation candidate**, while retaining
`gix` 0.86 as the fallback. Both expose all required low-level primitives, but
`git2` directly documents equivalents for several commands criv currently
runs, including the blended tree/index/worktree semantics of `git diff
<tree>`. It also reuses the `libgit2-sys` 0.18 line and vendored libgit2 already
present through `fff-search`; adopting `gix` would add a second Git
implementation without removing that native dependency.

This is a migration-risk conclusion, not a performance conclusion. Binary
size, build time, runtime cost, release portability, and exact behavioral
parity remain measurements. Adoption should be conditional on the differential
test matrix at the end of this note.

## Runtime operation matrix

| Contract area | `git2` 0.21 | `gix` 0.86 | Consequence for criv |
| --- | --- | --- | --- |
| Repository discovery | [`Repository::discover`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.discover) searches upward. The repository API exposes `is_bare`, `is_worktree`, `workdir`, and `commondir`. | [`gix::discover`](https://docs.rs/gix/0.86.0/gix/fn.discover.html) searches upward, and discovery models [normal, linked-worktree, and repository-only paths](https://docs.rs/gix/0.86.0/gix/discover/repository/enum.Path.html). | Either can use its plain discovery API, rather than an environment-aware variant, to preserve criv's deliberate removal of inherited `GIT_*` context. Bare repositories must continue to count as “not inside a worktree,” matching `rev-parse --is-inside-work-tree`. |
| HEAD to index | [`diff_tree_to_index`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.diff_tree_to_index) is documented as equivalent to `git diff --cached`; `None` represents an empty old tree. | [`tree_index_status`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.tree_index_status) provides the tree/index half of status. `index_or_load_from_head_or_empty` explicitly supports an absent index or unborn HEAD. | Both cover commit-stage enforcement. The adapter must select the empty tree for unborn HEAD. |
| Index to worktree | [`diff_index_to_workdir`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.diff_index_to_workdir) is documented as matching `git diff`. | [`Repository::status`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.status) and the [status platform](https://docs.rs/gix/0.86.0/gix/status/index.html) expose index/worktree changes behind the `status` feature. | Both can cover the unstaged half, with untracked files disabled because current `git diff` does not report them. |
| Tree to worktree through the index | [`diff_tree_to_workdir_with_index`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.diff_tree_to_workdir_with_index) explicitly blends tree-to-index and index-to-worktree results to emulate `git diff <tree>`. | The high-level status iterator exposes separate tree/index and index/worktree relations. Its docs do not promise a one-call blended `git diff <tree>` result. | `git2` maps directly to CI's `git diff HEAD`. A `gix` adapter would have to compose the two relations and prove net-result parity for staged changes later cancelled or changed again in the worktree. |
| Tree to tree and three-dot comparisons | [`diff_tree_to_tree`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.diff_tree_to_tree) is documented as equivalent to `git diff <old-tree> <new-tree>`; [`merge_base`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.merge_base) supplies the three-dot base. | Tree [`changes`](https://docs.rs/gix/0.86.0/gix/struct.Tree.html#method.changes) and repository [`merge_base`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.merge_base) provide the same primitives. | Either can cover manual push and CI comparisons after resolving each revision to a commit/tree. Root commits use an empty old tree. |
| Rename and copy handling | [`Diff::find_similar`](https://docs.rs/git2/0.21.0/git2/struct.Diff.html#method.find_similar) and [`DiffFindOptions`](https://docs.rs/git2/0.21.0/git2/struct.DiffFindOptions.html) support renames, copies, thresholds, rewrite splitting, and limits. | [`gix_diff::Rewrites`](https://docs.rs/gix/0.86.0/gix/diff/struct.Rewrites.html) supports rename/copy thresholds and limits. Tree/index status can read `status.renames` and `diff.renames`; index/worktree rewrites are configured separately. | Neither API call alone proves parity with the user's Git configuration. The adapter must deliberately reproduce Git defaults and config precedence, including `diff.renames=copies`, and preserve old/new path orientation. |
| Blob at a revision | [`revparse_single`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.revparse_single), tree lookup, and [`find_blob`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.find_blob) replace `git show <rev>:<path>`. | [`rev_parse_single`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.rev_parse_single), tree lookup, and [`find_blob`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.find_blob) expose the same object path. | Either supports query snapshots and old/new ADR content without a subprocess. The adapter must reject a missing path, wrong object kind, or invalid UTF-8 with criv-compatible outcomes. |
| Blob from the index | [`Index::get_path`](https://docs.rs/git2/0.21.0/git2/struct.Index.html#method.get_path) yields an entry object ID for `find_blob`. | Repository index entries use byte paths and expose object IDs for `find_blob`. | Both replace `git show :<path>`. Stage-zero selection and conflicted multi-stage entries need explicit tests. |
| Revision traversal | [`Revwalk`](https://docs.rs/git2/0.21.0/git2/struct.Revwalk.html) supports pushed tips, hidden commits/refs/globs, sort flags, reverse order, and first-parent simplification. | [`rev_walk`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.rev_walk) supports tips, [`with_hidden`](https://docs.rs/gix/0.86.0/gix/revision/walk/struct.Platform.html#method.with_hidden), sorting, first-parent-only traversal, commit graphs, and shallow boundaries. | For an existing remote ref, start at the local OID and hide the remote OID. For a new remote ref, hide all commit tips under `refs/remotes/<remote>/`. The exact order produced by `rev-list --reverse` still needs differential testing for either backend. |
| First parent | [`Commit::parent_id(0)`](https://docs.rs/git2/0.21.0/git2/struct.Commit.html#method.parent_id) returns the first parent and reports absence on a root commit. | [`Commit::parent_ids`](https://docs.rs/gix/0.86.0/gix/struct.Commit.html#method.parent_ids) preserves encoded parent order. | Both replace `<commit>^`; a root maps to `None`. |
| Linked worktrees | Repository discovery exposes worktree identity and a shared common directory; the API can also enumerate and open [worktrees](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.worktrees). | Opening a linked worktree resolves its `commondir`; repository APIs expose [`common_dir`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.common_dir) and linked [`worktrees`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.worktrees). | Both model linked worktrees. Discovery, index selection, HEAD, and remote-ref visibility must be tested from the linked checkout itself. |
| Unborn HEAD | `head` reports [`ErrorCode::UnbornBranch`](https://docs.rs/git2/0.21.0/git2/enum.ErrorCode.html#variant.UnbornBranch), and the diff APIs accept an empty tree. | [`Head::is_unborn`](https://docs.rs/gix/0.86.0/gix/struct.Head.html#method.is_unborn) and empty-index/head helpers model the state explicitly. | Either can preserve staged-file behavior before the first commit; manual push and revision reads should retain their current failure/fallback semantics. |
| Shallow repositories | [`is_shallow`](https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.is_shallow) exposes the state; libgit2 represents shallow roots as graft boundaries internally. | [`is_shallow` and `shallow_commits`](https://docs.rs/gix/0.86.0/gix/struct.Repository.html#method.shallow_commits) expose boundaries, and the revision walker explicitly observes them. | Both acknowledge shallow state, but only fixture comparison can establish the same outgoing set and missing-parent behavior as the installed Git. |
| Path encoding | Diff/status entries expose raw bytes, including [`StatusEntry::path_bytes`](https://docs.rs/git2/0.21.0/git2/struct.StatusEntry.html#method.path_bytes); index entries also store byte paths. | Status locations and repository-relative paths are generally `BStr`/`BString`; the [status item implementation](https://docs.rs/gix/0.86.0/src/gix/status/iter/types.rs.html) retains byte paths. | Both can avoid lossy conversion. Criv should continue its present explicit UTF-8 validation rather than silently accepting or replacing invalid bytes. |
| Errors | [`git2::Error`](https://docs.rs/git2/0.21.0/git2/struct.Error.html) contains a code, class, and message. | Operations expose typed, source-chained error enums, such as [`gix::status::Error`](https://docs.rs/gix/0.86.0/gix/status/enum.Error.html). | Neither reproduces the Git executable's exit status and stderr. A criv-owned compatibility layer must normalize errors and preserve intentional fallbacks; exact wording can only be settled by existing tests or an explicit compatibility decision. |

Two documented `gix` differences deserve special attention. Its status API
performs the index-modified check and directory walk in parallel rather than
Git's processing order. Its index/worktree rewrite API also configures sorting
to obtain deterministic rewrite solutions. These do not disqualify `gix`, but
they make changed-entry ordering an explicit adapter concern.

## Dependency, build, and release comparison

The current lock contains this path:

```text
git2 0.20.4 <- fff-search 0.10.1 <- criv
libgit2-sys 0.18.7+1.9.6 <- git2 0.20.4
```

`fff-search` disables `git2` defaults and enables `vendored-libgit2`. The
current build therefore already compiles and statically links vendored
libgit2. The [git2-rs build policy](https://docs.rs/crate/git2/0.21.0/source/README.md)
documents the system-library probe and vendored fallback; the
[`libgit2-sys` build script](https://docs.rs/crate/libgit2-sys/0.18.7+1.9.6/source/build.rs)
shows that the fallback uses a C compiler and platform-specific source sets.

`git2` 0.21 has no default features, so criv's local operations require no SSH
or HTTPS stack. It requires `libgit2-sys ^0.18.4`, while `git2` 0.20.4 requires
`^0.18.3`; Cargo can resolve both wrappers onto the existing 0.18.7 package.
Cargo also permits only one package with a given native `links` value in a
dependency graph, as documented by the [Cargo resolver](https://doc.rust-lang.org/cargo/reference/resolver.html#links).
The two `git2` Rust wrapper versions would nevertheless coexist until
`fff-search` upgrades. A trial lockfile is still required to confirm the exact
resolved graph.

`gix` is a [pure-Rust Git implementation](https://github.com/GitoxideLabs/gitoxide/tree/gix-v0.86.0),
but selecting it would not make criv's complete dependency graph pure Rust
while `fff-search` still brings vendored libgit2. Its defaults enable a broad
bundle. The [0.86 feature manifest](https://docs.rs/crate/gix/0.86.0/source/Cargo.toml.orig)
recommends that library users disable defaults and choose only required
components. Criv would at least need SHA-1, revision, index, blob-diff, and
status-related functionality; `status` itself pulls directory walking,
attributes, excludes, and index diffing. The selected feature closure must be
confirmed by an actual candidate manifest.

`gix` 0.86 declares Rust 1.85; `git2` 0.21 declares no `rust-version`. Both
Rust crates are MIT OR Apache-2.0. Vendored libgit2 is GPLv2 with an explicit
[linking exception](https://github.com/libgit2/libgit2/blob/main/COPYING); that
license is already present in criv's shipped dependency graph.

Both projects show current upstream activity as of 2026-08-02:
[`git2-rs`](https://github.com/rust-lang/git2-rs/commits/main/) had a main-branch
commit on 2026-07-25, while [`gix` 0.86](https://github.com/GitoxideLabs/gitoxide/releases/tag/gix-v0.86.0)
was released on 2026-07-23 and the Gitoxide repository remained active on
2026-08-01. Activity is a point-in-time fact, not a measure of future
maintenance cost.

## Security history

`git2` has direct and `libgit2-sys` advisory history. Version 0.21 patches the
two unsoundness advisories that affect 0.20.4:
[RUSTSEC-2026-0183](https://rustsec.org/advisories/RUSTSEC-2026-0183.html)
and [RUSTSEC-2026-0184](https://rustsec.org/advisories/RUSTSEC-2026-0184.html).
Adding 0.21 would not remove the transitive 0.20.4 wrapper, so those audit
warnings remain until `fff-search` upgrades even though new criv calls use the
patched version. Earlier Git transport and libgit2 issues are patched in the
resolved lines; network features are unnecessary for this runtime contract.

Gitoxide also has historical advisories across component crates. Gix 0.86's
declared versions are above the fixes for the relevant status/path components,
including [gix-attributes](https://rustsec.org/advisories/RUSTSEC-2024-0359.html),
[gix-worktree](https://rustsec.org/advisories/RUSTSEC-2024-0353.html), and
[gix-worktree-state](https://rustsec.org/advisories/RUSTSEC-2025-0001.html).
This is not proof that a future selected feature closure is advisory-free; the
resolved candidate lockfile must be audited.

Security history does not distinguish either project as risk-free. The useful
near-term distinction is narrower: direct use of `git2` should start at 0.21,
not reuse the advisory-bearing 0.20.4 Rust API.

## Measurements still required

Primary sources establish API availability and documented semantics. They do
not establish the following claims, which must not be used as decision facts
until measured:

- release binary-size delta for either candidate under criv's size-optimized
  profile;
- clean and incremental build-time delta, resolved crate count, and release
  packaging reliability;
- runtime latency, peak memory, or comparative performance;
- cross-target reliability on Linux, macOS, and Windows;
- exact changed-entry ordering and status blending;
- rename/copy parity under defaults, `diff.renames`, `status.renames`, custom
  thresholds, limits, and copies mode;
- `rev-list --reverse` order for merges and equal timestamps;
- linked-worktree, unborn-HEAD, shallow-boundary, missing-object, replace-ref,
  and graft behavior;
- current audit results for the newly resolved dependency closure;
- user-visible error text and the current manual-push fallback behavior; and
- the absence of `git` executable launches across every enabled runtime path.

## Required differential test matrix

Before selecting the backend, run both candidate adapters against the current
Git CLI and compare normalized `ChangedSet`, blob, traversal, and error results.
The fixtures should cover:

- normal, detached-HEAD, unborn, bare, shallow, and linked-worktree
  repositories;
- staged-only, unstaged-only, mixed, deleted, type-changed, conflicted, and
  staged-then-cancelled worktree changes;
- rename and copy settings, limits, thresholds, and non-UTF-8 paths on Unix;
- two-parent merges, root commits, equal timestamps, existing remote updates,
  new remote branches, deleted updates, and several remote-tracking tips;
- missing refs, paths, blobs, and shallow parents, including existing fallback
  paths; and
- a poisoned and an absent `git` executable in `PATH`, proving the shipped
  runtime never launches it.

Measure stripped release size, clean/incremental build duration, runtime and
peak memory on representative repositories, and the repository's actual
release target matrix. These results should decide whether `git2`'s lower
semantic migration risk outweighs any measured cost; they are not prerequisites
for using `git2` as the first candidate.

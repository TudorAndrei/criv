//! The transaction that both reconcile commands run: plan, check, write,
//! validate, stage, prove, and commit, restoring the start state on failure.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::check;
use crate::diagnostic::Diagnostic;
use crate::git::{ChangedEntry, Comparison, GitRepository};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

use super::receipt::Receipt;

/// Whether a reconcile command only reports or also commits.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Mode {
    Check,
    Write,
}

/// One reconcile run's repository and the commits it compares.
pub(super) struct Context<'a> {
    pub(super) files: &'a RepositoryFiles,
    pub(super) git: &'a GitRepository,
    pub(super) base_ref: &'a str,
    pub(super) target_sha: &'a str,
    pub(super) head_sha: &'a str,
    pub(super) merge_base: &'a str,
}

/// The files a transaction writes and the paths it deletes.
#[derive(Default)]
pub(super) struct Writes {
    pub(super) files: BTreeMap<String, FileWrite>,
    pub(super) deletions: Vec<String>,
}

pub(super) struct FileWrite {
    pub(super) contents: String,
    /// The permissions to publish with; `None` keeps the current ones.
    pub(super) permissions: Option<fs::Permissions>,
}

impl Writes {
    fn paths(&self) -> Vec<String> {
        self.files
            .keys()
            .chain(&self.deletions)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// What differs between the two reconcile commands.
pub(super) trait Reconciliation {
    type Plan;
    type Receipt: Receipt;
    const COMMAND: &'static str;
    const COMMIT_MESSAGE: &'static str;
    /// The message when nothing needs reconciliation.
    const CURRENT: &'static str;

    fn plan(context: &Context<'_>) -> Result<Self::Plan>;
    fn is_current(plan: &Self::Plan) -> bool;
    fn print(plan: &Self::Plan);
    /// The `--check` failure for a plan that is not current.
    fn conflict(context: &Context<'_>, plan: &Self::Plan) -> String;
    /// Whether a dirty worktree is this plan's own materialized transaction.
    fn resumes(plan: &Self::Plan) -> bool;
    fn writes(context: &Context<'_>, plan: &Self::Plan) -> Result<Writes>;
    /// The receipt of the written transaction.
    fn receipt(context: &Context<'_>, plan: &Self::Plan, writes: &Writes) -> Result<Self::Receipt>;
    /// Checks the written worktree before its staged proof.
    fn verify_written(_context: &Context<'_>) -> Result<()> {
        Ok(())
    }
    fn proves(receipt: &Self::Receipt, git: &GitRepository, staged: &[ChangedEntry]) -> bool;
}

pub(super) fn run<R: Reconciliation>(
    files: &RepositoryFiles,
    base_ref: &str,
    mode: Mode,
) -> Result<()> {
    let name = <R::Receipt as Receipt>::NAME;
    let git = GitRepository::discover(files.root())?
        .ok_or_else(|| CrivError::new(format!("`{}` requires a Git worktree", R::COMMAND)))?;
    let target_sha = git.resolve_commit(base_ref).map_err(|_| {
        CrivError::new(format!(
            "cannot resolve base ref `{base_ref}` to a commit; fetch complete history and retry"
        ))
    })?;
    let head_sha = git.resolve_commit("HEAD")?;
    let merge_base = git
        .merge_base(&target_sha, &head_sha)
        .map_err(|error| CrivError::new(format!("{error}; fetch complete history and retry")))?;
    let context = Context {
        files,
        git: &git,
        base_ref,
        target_sha: &target_sha,
        head_sha: &head_sha,
        merge_base: &merge_base,
    };
    let plan = R::plan(&context)?;
    println!("{name} target: {target_sha}");
    if R::is_current(&plan) {
        println!("{}", R::CURRENT);
        return Ok(());
    }
    R::print(&plan);
    if mode == Mode::Check {
        return Err(CrivError::new(format!(
            "{}; run `{} --base {base_ref}`",
            R::conflict(&context, &plan),
            R::COMMAND
        )));
    }
    let dirty = git.dirty_paths()?;
    if !dirty.is_empty() && !R::resumes(&plan) {
        return Err(CrivError::new(format!(
            "refusing to reconcile a dirty worktree; commit or stash: {}",
            dirty.join(", ")
        )));
    }
    git.preflight_commit_identity()
        .map_err(|error| CrivError::new(format!("{error}; {name} commits its own transaction")))?;
    ensure_stable(&context, &format!("before {name}"))?;
    let writes = R::writes(&context, &plan)?;
    let paths = writes.paths();
    let snapshot = Snapshot::capture(
        files,
        &git,
        &paths
            .iter()
            .map(String::as_str)
            .chain([<R::Receipt as Receipt>::PATH])
            .collect::<Vec<_>>(),
    )?;
    let committed = (|| {
        apply(files, &writes)?;
        validate(files, name)?;
        let receipt = R::receipt(&context, &plan, &writes)?;
        let contents = serde_json::to_string_pretty(&receipt)
            .map_err(|error| CrivError::new(format!("cannot serialize {name} receipt: {error}")))?
            + "\n";
        files
            .write_scope(Path::new(criv_state_wire::layout::DIR))?
            .write_atomic(Path::new(<R::Receipt as Receipt>::PATH), &contents)?;
        ensure_stable(&context, &format!("during {name}"))?;
        git.stage_paths(&paths)?;
        R::verify_written(&context)?;
        let staged = git.changed_set(&Comparison::Staged, &[])?;
        if !R::proves(&receipt, &git, &staged.entries) {
            return Err(CrivError::new(format!(
                "{name} receipt does not prove the complete staged transaction"
            )));
        }
        ensure_stable(&context, &format!("before the {name} commit"))?;
        git.commit_staged(R::COMMIT_MESSAGE)
    })();
    match committed {
        Ok(commit) => {
            println!("{name} committed: {commit}");
            Ok(())
        }
        Err(error) => {
            let rollback_errors = snapshot.rollback();
            Err(if rollback_errors.is_empty() {
                error
            } else {
                CrivError::new(format!(
                    "{error}\n{name} rollback also failed:\n{}",
                    rollback_errors.join("\n")
                ))
            })
        }
    }
}

fn ensure_stable(context: &Context<'_>, moment: &str) -> Result<()> {
    if super::receipt::target_is_stable(context.git, context.base_ref, context.target_sha) {
        Ok(())
    } else {
        Err(CrivError::new(format!(
            "target ref `{}` moved {moment}; retry against its new SHA",
            context.base_ref
        )))
    }
}

fn apply(files: &RepositoryFiles, writes: &Writes) -> Result<()> {
    let scope = files.write_scope(Path::new("."))?;
    for (path, write) in &writes.files {
        match &write.permissions {
            Some(permissions) => scope.write_atomic_with_permissions(
                Path::new(path),
                write.contents.as_bytes(),
                permissions.clone(),
            )?,
            None => scope.write_atomic(Path::new(path), &write.contents)?,
        }
    }
    for path in &writes.deletions {
        // A retry may find a path its earlier materialized receipt removed.
        if files.file_exists(Path::new(path))? {
            scope.remove_file(Path::new(path))?;
        }
    }
    Ok(())
}

fn validate(files: &RepositoryFiles, name: &str) -> Result<()> {
    let errors = check::validate_all_from(files)?
        .into_iter()
        .filter(Diagnostic::is_error)
        .map(|diagnostic| diagnostic.describe())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        return Ok(());
    }
    Err(CrivError::new(format!(
        "{name} wrote files but vault validation failed:\n{}",
        errors.join("\n")
    )))
}

/// The Git index and every path a transaction may change, captured before
/// the first write.
struct Snapshot {
    files: RepositoryFiles,
    index: IndexSnapshot,
    paths: Vec<CapturedPath>,
}

struct CapturedPath {
    path: String,
    contents: Option<(Vec<u8>, fs::Permissions)>,
}

/// The raw index file, restored under Git's own `index.lock` so a concurrent
/// Git process is never overwritten.
struct IndexSnapshot {
    git_files: RepositoryFiles,
    contents: Option<(Vec<u8>, fs::Permissions)>,
}

const INDEX: &str = "index";
const INDEX_LOCK: &str = "index.lock";

impl Snapshot {
    fn capture(files: &RepositoryFiles, git: &GitRepository, paths: &[&str]) -> Result<Self> {
        Ok(Self {
            files: files.clone(),
            index: IndexSnapshot::capture(git)?,
            paths: paths
                .iter()
                .map(|path| {
                    Ok(CapturedPath {
                        path: (*path).to_owned(),
                        contents: files.read_optional_with_permissions(Path::new(path))?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }

    /// Restores everything it can and returns every failure.
    fn rollback(self) -> Vec<String> {
        let mut errors = Vec::new();
        if let Err(error) = self.index.restore() {
            errors.push(error.to_string());
        }
        for path in self.paths {
            if let Err(error) = path.restore(&self.files) {
                errors.push(error.to_string());
            }
        }
        errors
    }
}

impl CapturedPath {
    fn restore(self, files: &RepositoryFiles) -> Result<()> {
        let scope = files.write_scope(Path::new("."))?;
        let relative = Path::new(&self.path);
        match self.contents {
            Some((contents, permissions)) => {
                scope.write_atomic_with_permissions(relative, &contents, permissions)
            }
            None if files.file_exists(relative)? => scope.remove_file(relative),
            None => Ok(()),
        }
    }
}

impl IndexSnapshot {
    fn capture(git: &GitRepository) -> Result<Self> {
        let git_files = RepositoryFiles::open(git.git_dir())?;
        let contents = git_files
            .read_optional_with_permissions(Path::new(INDEX))
            .map_err(|error| {
                CrivError::new(format!("Git index could not be snapshotted: {error}"))
            })?;
        Ok(Self {
            git_files,
            contents,
        })
    }

    fn restore(self) -> Result<()> {
        let scope = self.git_files.write_scope(Path::new("."))?;
        let lock = Path::new(INDEX_LOCK);
        let mut lock_file = scope
            .create_new(lock)
            .map_err(|error| CrivError::new(format!("Git index could not be restored: {error}")))?;
        let restored = (|| -> Result<()> {
            if let Some((contents, permissions)) = self.contents {
                lock_file.write_all(&contents)?;
                lock_file.set_permissions(permissions)?;
                lock_file.sync_all()?;
                drop(lock_file);
                return scope.rename_file(lock, Path::new(INDEX));
            }
            drop(lock_file);
            if self.git_files.file_exists(Path::new(INDEX))? {
                scope.remove_file(Path::new(INDEX))?;
            }
            scope.remove_file(lock)
        })();
        restored.map_err(|error| {
            let error = format!("Git index could not be restored: {error}");
            let cleanup = match self.git_files.file_exists(lock) {
                Ok(false) => Ok(()),
                _ => scope.remove_file(lock),
            };
            CrivError::new(match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; removing {INDEX_LOCK} also failed: {cleanup}"),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::Snapshot;
    use crate::git::GitRepository;
    use crate::git::fixture::{git, git_with_input};
    use crate::repository::RepositoryFiles;

    fn capture(root: &Path, paths: &[&str]) -> Snapshot {
        let files = RepositoryFiles::open(root).unwrap();
        let repository = GitRepository::open(root).unwrap();
        Snapshot::capture(&files, &repository, paths).unwrap()
    }

    #[test]
    fn rollback_restores_the_index_existing_files_and_absent_files() {
        let temp = repository();
        let root = temp.path();
        let existing = "docs/existing.md";
        let receipt = ".criv/reconcile.json";
        fs::create_dir_all(root.join(".criv")).unwrap();
        fs::write(root.join(existing), "staged\n").unwrap();
        #[cfg(unix)]
        set_mode(&root.join(existing), 0o640);
        git(root, &["add", existing]);
        let index_before = git(root, &["ls-files", "--stage"]);

        let snapshot = capture(root, &[existing, receipt]);
        fs::write(root.join(existing), "changed\n").unwrap();
        #[cfg(unix)]
        set_mode(&root.join(existing), 0o755);
        fs::write(root.join(receipt), "new receipt\n").unwrap();
        git(root, &["add", "-A"]);

        assert!(snapshot.rollback().is_empty());
        assert_eq!(fs::read_to_string(root.join(existing)).unwrap(), "staged\n");
        assert!(!root.join(receipt).exists());
        assert_eq!(git(root, &["ls-files", "--stage"]), index_before);
        #[cfg(unix)]
        assert_eq!(mode(&root.join(existing)), 0o640);
    }

    #[test]
    fn rollback_continues_after_one_path_cannot_be_restored() {
        let temp = repository();
        let root = temp.path();
        let blocked = "docs/blocked.md";
        let later = "docs/later.md";
        fs::write(root.join(blocked), "blocked before\n").unwrap();
        fs::write(root.join(later), "later before\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "test: add rollback paths"]);
        let snapshot = capture(root, &[blocked, later]);

        fs::remove_file(root.join(blocked)).unwrap();
        fs::create_dir(root.join(blocked)).unwrap();
        fs::write(root.join(later), "later changed\n").unwrap();

        let errors = snapshot.rollback();

        assert!(!errors.is_empty());
        assert!(root.join(blocked).is_dir());
        assert_eq!(
            fs::read_to_string(root.join(later)).unwrap(),
            "later before\n"
        );
    }

    #[test]
    fn rollback_restores_index_only_flags_with_a_clean_worktree() {
        let temp = repository();
        let root = temp.path();
        fs::write(root.join("assume.txt"), "assume unchanged\n").unwrap();
        fs::write(root.join("skip.txt"), "skip worktree\n").unwrap();
        git(root, &["add", "assume.txt", "skip.txt"]);
        git(root, &["commit", "-qm", "test: add flagged files"]);
        git(root, &["update-index", "--assume-unchanged", "assume.txt"]);
        git(root, &["update-index", "--skip-worktree", "skip.txt"]);
        assert!(git(root, &["status", "--porcelain=v1"]).is_empty());
        let flags = |root| git(root, &["ls-files", "-v", "--", "assume.txt", "skip.txt"]);
        let flags_before = flags(root);
        assert!(flags_before.lines().any(|line| line == "h assume.txt"));
        assert!(flags_before.lines().any(|line| line == "S skip.txt"));
        let index_before = fs::read(root.join(".git/index")).unwrap();

        let snapshot = capture(root, &[]);
        git(
            root,
            &["update-index", "--no-assume-unchanged", "assume.txt"],
        );
        git(root, &["update-index", "--no-skip-worktree", "skip.txt"]);
        assert_ne!(flags(root), flags_before);

        assert!(snapshot.rollback().is_empty());
        assert_eq!(flags(root), flags_before);
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
        assert!(git(root, &["status", "--porcelain=v1"]).is_empty());
    }

    #[test]
    fn rollback_refuses_to_replace_an_index_with_an_active_git_lock() {
        let temp = repository();
        let root = temp.path();
        let snapshot = capture(root, &[]);
        fs::write(root.join("new.txt"), "new\n").unwrap();
        git(root, &["add", "new.txt"]);
        let changed_index = fs::read(root.join(".git/index")).unwrap();
        fs::write(
            root.join(".git/index.lock"),
            "held by another Git process\n",
        )
        .unwrap();

        let errors = snapshot.rollback();

        assert!(
            errors.iter().any(|error| error.contains("index.lock")),
            "rollback must report the active Git lock: {errors:?}"
        );
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), changed_index);
        assert!(root.join(".git/index.lock").exists());
    }

    #[test]
    fn rollback_restores_conflict_stages() {
        let temp = repository();
        let root = temp.path();
        let object = |contents| git_with_input(root, &["hash-object", "-w", "--stdin"], contents);
        let entries = format!(
            "100644 {} 1\tconflict.txt\n100644 {} 2\tconflict.txt\n100644 {} 3\tconflict.txt\n",
            object("base\n"),
            object("ours\n"),
            object("theirs\n")
        );
        git_with_input(root, &["update-index", "--index-info"], &entries);
        let stages = || git(root, &["ls-files", "--stage", "--", "conflict.txt"]);
        let stages_before = stages();
        for stage in [" 1\t", " 2\t", " 3\t"] {
            assert!(stages_before.contains(&format!("{stage}conflict.txt")));
        }
        let index_before = fs::read(root.join(".git/index")).unwrap();

        let snapshot = capture(root, &[]);
        git(root, &["reset", "--mixed", "HEAD"]);
        assert!(stages().is_empty());

        assert!(snapshot.rollback().is_empty());
        assert_eq!(stages(), stages_before);
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
    }

    fn repository() -> TempDir {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/existing.md"), "committed\n").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "test: initialize transaction"]);
        temp
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
}

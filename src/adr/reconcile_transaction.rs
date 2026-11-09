//! The transaction that both reconcile commands run: plan, check, write,
//! validate, stage, prove, and commit, restoring the start state on failure.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::check;
use crate::diagnostic::Diagnostic;
use crate::git::{ChangedSet, Comparison, GitRepository, IndexSnapshot};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

use super::receipt::{Receipt, ReceiptHeader};

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
    base_ref: &'a str,
    pub(super) target_sha: String,
    pub(super) head_sha: String,
    pub(super) merge_base: String,
    /// Every change from `merge_base` to `head_sha`.
    pub(super) history: ChangedSet,
}

impl<'a> Context<'a> {
    fn open(files: &'a RepositoryFiles, git: &'a GitRepository, base_ref: &'a str) -> Result<Self> {
        let target_sha = git.resolve_commit(base_ref).map_err(|_| {
            CrivError::new(format!(
                "cannot resolve base ref `{base_ref}` to a commit; fetch complete history and retry"
            ))
        })?;
        let head_sha = git.resolve_commit("HEAD")?;
        let merge_base = git.merge_base(&target_sha, &head_sha).map_err(|error| {
            CrivError::new(format!("{error}; fetch complete history and retry"))
        })?;
        let history = git.changed_set(
            &Comparison::Trees {
                old_ref: &merge_base,
                new_ref: &head_sha,
            },
            &[],
        )?;
        Ok(Self {
            files,
            git,
            base_ref,
            target_sha,
            head_sha,
            merge_base,
            history,
        })
    }

    /// The header of a receipt of this run.
    pub(super) fn receipt_header<R: Receipt>(&self) -> ReceiptHeader {
        ReceiptHeader {
            schema: R::SCHEMA.to_owned(),
            base_ref: self.base_ref.to_owned(),
            head_sha: self.head_sha.clone(),
            target_sha: self.target_sha.clone(),
            merge_base: self.merge_base.clone(),
        }
    }
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
}

/// The `--check` failure for `base_ref`, or `None` when nothing needs
/// reconciliation. Nothing is printed.
pub(super) fn conflict<R: Reconciliation>(
    files: &RepositoryFiles,
    git: &GitRepository,
    base_ref: &str,
) -> Result<Option<String>> {
    let context = Context::open(files, git, base_ref)?;
    let plan = R::plan(&context)?;
    Ok((!R::is_current(&plan)).then(|| check_failure::<R>(&context, &plan)))
}

fn check_failure<R: Reconciliation>(context: &Context<'_>, plan: &R::Plan) -> String {
    format!(
        "{}; run `{} --base {}`",
        R::conflict(context, plan),
        R::COMMAND,
        context.base_ref
    )
}

pub(super) fn run<R: Reconciliation>(
    files: &RepositoryFiles,
    base_ref: &str,
    mode: Mode,
) -> Result<()> {
    let name = <R::Receipt as Receipt>::NAME;
    let repository = GitRepository::discover(files.root())?
        .ok_or_else(|| CrivError::new(format!("`{}` requires a Git worktree", R::COMMAND)))?;
    let git = &repository;
    let context = Context::open(files, git, base_ref)?;
    let plan = R::plan(&context)?;
    println!("{name} target: {}", context.target_sha);
    if R::is_current(&plan) {
        println!("{}", R::CURRENT);
        return Ok(());
    }
    R::print(&plan);
    if mode == Mode::Check {
        return Err(CrivError::new(check_failure::<R>(&context, &plan)));
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
        git,
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
        if !receipt.allows_transaction(git, &staged.entries) {
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
    if super::receipt::target_is_stable(context.git, context.base_ref, &context.target_sha) {
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

impl Snapshot {
    fn capture(files: &RepositoryFiles, git: &GitRepository, paths: &[&str]) -> Result<Self> {
        Ok(Self {
            files: files.clone(),
            index: git.snapshot_index()?,
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::Snapshot;
    use crate::git::GitRepository;
    use crate::git::fixture::git;
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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::snapshots;
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

const LOCK_PATH: &str = ".criv/state-publication.lock";
const TRANSACTION_PATH: &str = ".criv/state-transaction.json";
const STAGE_DIR: &str = ".criv/state-stage";
const QUARANTINE_DIR: &str = ".criv/state-quarantine";
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const LOCK_RETRY: Duration = Duration::from_millis(25);
const TRANSACTION_SCHEMA: &str = "criv.state-publication.v1";

#[derive(Debug, Serialize, Deserialize)]
struct TransactionRecord {
    schema: String,
    phase: TransactionPhase,
    candidate_hash: String,
    candidate_state: String,
    candidate_index: String,
    keep: usize,
    removals: Vec<String>,
    prior_state: Option<String>,
    prior_latest: Option<String>,
    prior_index: Option<String>,
    prior_snapshots: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Prepared,
    Staged,
    Quarantined,
    Installed,
    Committed,
    Cleanup,
}

struct PublicationLock {
    _file: fs::File,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PublicationStep {
    Preflight,
    Record,
    StageSnapshot,
    StageIndex,
    StageLatest,
    Quarantine,
    InstallSnapshot,
    InstallIndex,
    InstallLatest,
    BeforeCommit,
    Commit,
    Cleanup,
    Rollback,
}

trait PublicationControl {
    fn checkpoint(&self, _step: PublicationStep) -> Result<()> {
        Ok(())
    }
}

struct RealPublicationControl;

impl PublicationControl for RealPublicationControl {}

#[cfg(test)]
fn publish(root: &Path, hash: &str, contents: &str, keep: usize) -> Result<()> {
    publish_with(root, hash, contents, keep, &RealPublicationControl)
}

pub(crate) fn publish_with_precommit_check(
    files: &RepositoryFiles,
    hash: &str,
    contents: &str,
    keep: usize,
    precommit_check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    publish_with_check(
        files,
        hash,
        contents,
        keep,
        &RealPublicationControl,
        precommit_check,
    )
}

#[cfg(test)]
fn publish_with(
    root: &Path,
    hash: &str,
    contents: &str,
    keep: usize,
    control: &impl PublicationControl,
) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    publish_with_check(&files, hash, contents, keep, control, || Ok(()))
}

fn publish_with_check(
    files: &RepositoryFiles,
    hash: &str,
    contents: &str,
    keep: usize,
    control: &impl PublicationControl,
    precommit_check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let _lock = PublicationLock::acquire(files)?;
    recover_locked(files)?;
    reject_orphan_transaction_workspace(files)?;
    let plan = snapshots::plan_publication(files, hash, contents, keep)?;
    control.checkpoint(PublicationStep::Preflight)?;
    let mut record = TransactionRecord {
        schema: TRANSACTION_SCHEMA.to_string(),
        phase: TransactionPhase::Prepared,
        candidate_hash: hash.to_string(),
        candidate_state: contents.to_string(),
        candidate_index: plan.index_contents,
        keep,
        removals: plan.removals,
        prior_state: read_optional(files, ".criv/state.json", "State commit record")?,
        prior_latest: read_optional(files, ".criv/latest", "latest snapshot pointer")?,
        prior_index: read_optional(files, ".criv/snapshots/index.json", "snapshot index")?,
        prior_snapshots: capture_snapshots(files)?,
    };
    write_record(files, &record)?;
    if let Err(error) = control.checkpoint(PublicationStep::Record) {
        cleanup_transaction(files, &record)?;
        return Err(error);
    }

    let result = stage_candidate(files, &record, control)
        .and_then(|_| set_phase(files, &mut record, TransactionPhase::Staged))
        .and_then(|_| quarantine_removals(files, &record, control))
        .and_then(|_| set_phase(files, &mut record, TransactionPhase::Quarantined))
        .and_then(|_| install_candidate_controlled(files, &record, control))
        .and_then(|_| set_phase(files, &mut record, TransactionPhase::Installed))
        .and_then(|_| control.checkpoint(PublicationStep::BeforeCommit))
        .and_then(|_| precommit_check())
        .and_then(|_| {
            files
                .write_scope(Path::new(".criv"))?
                .write_atomic(Path::new(".criv/state.json"), contents)
        })
        .and_then(|_| set_phase(files, &mut record, TransactionPhase::Committed))
        .and_then(|_| control.checkpoint(PublicationStep::Commit));
    if let Err(error) = result {
        let state_committed = read_optional(files, ".criv/state.json", "State commit record")?
            .as_deref()
            == Some(contents);
        if state_committed {
            eprintln!("criv: warning: State publication cleanup was interrupted: {error}");
            return Ok(());
        }
        if let Err(rollback) = control.checkpoint(PublicationStep::Rollback) {
            return Err(CrivError::new(format!(
                "State publication failed: {error}; rollback failed: {rollback}; recovery is required"
            )));
        }
        if let Err(rollback) = restore_prior(files, &record) {
            return Err(CrivError::new(format!(
                "State publication failed: {error}; rollback failed: {rollback}; recovery is required"
            )));
        }
        cleanup_transaction(files, &record)?;
        return Err(error);
    }

    if let Err(error) = set_phase(files, &mut record, TransactionPhase::Cleanup)
        .and_then(|_| control.checkpoint(PublicationStep::Cleanup))
        .and_then(|_| cleanup_transaction(files, &record))
    {
        eprintln!("criv: warning: State publication cleanup failed: {error}");
    }
    Ok(())
}

fn reject_orphan_transaction_workspace(files: &RepositoryFiles) -> Result<()> {
    for relative in [STAGE_DIR, QUARANTINE_DIR] {
        if files.entry_exists(Path::new(relative))? {
            return Err(CrivError::new(format!(
                "State transaction workspace `{relative}` exists without a transaction record"
            )));
        }
    }
    Ok(())
}

fn set_phase(
    files: &RepositoryFiles,
    record: &mut TransactionRecord,
    phase: TransactionPhase,
) -> Result<()> {
    record.phase = phase;
    write_record(files, record)
}

pub(crate) fn load_snapshot(root: &Path, id: &str) -> Result<Option<String>> {
    let files = RepositoryFiles::open(root)?;
    let _lock = PublicationLock::acquire(&files)?;
    recover_locked(&files)?;
    snapshots::load_unlocked(&files, id)
}

impl PublicationLock {
    fn acquire(files: &RepositoryFiles) -> Result<Self> {
        let (_, file) = files
            .write_scope(Path::new(".criv"))?
            .open_regular_file(Path::new(LOCK_PATH))
            .map_err(|error| {
                CrivError::new(format!("unsafe State publication lock path: {error}"))
            })?;
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(LOCK_RETRY);
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(CrivError::new(
                        "timed out waiting 2 seconds for the State publication lock",
                    ));
                }
                Err(error) => {
                    return Err(CrivError::new(format!(
                        "failed to acquire operating-system State publication lock: {error}"
                    )));
                }
            }
        }
    }
}

fn recover_locked(files: &RepositoryFiles) -> Result<()> {
    let Some(contents) = read_optional(files, TRANSACTION_PATH, "State transaction record")? else {
        return Ok(());
    };
    let record: TransactionRecord = serde_json::from_str(&contents)
        .map_err(|error| CrivError::new(format!("State transaction record is corrupt: {error}")))?;
    if record.schema != TRANSACTION_SCHEMA {
        return Err(CrivError::new(format!(
            "unsupported State transaction schema `{}`",
            record.schema
        )));
    }

    let current = read_optional(files, ".criv/state.json", "State commit record")?;
    if current.as_deref() == Some(record.candidate_state.as_str()) {
        install_candidate(files, &record)?;
        cleanup_transaction(files, &record)?;
        return Ok(());
    }
    if current == record.prior_state {
        restore_prior(files, &record)?;
        cleanup_transaction(files, &record)?;
        return Ok(());
    }
    Err(CrivError::new(
        "State transaction recovery found an unknown State commit record",
    ))
}

fn write_record(files: &RepositoryFiles, record: &TransactionRecord) -> Result<()> {
    let contents = serde_json::to_string_pretty(record).map_err(|error| {
        CrivError::new(format!("failed to serialize State transaction: {error}"))
    })?;
    files
        .write_scope(Path::new(".criv"))?
        .write_atomic(Path::new(TRANSACTION_PATH), &format!("{contents}\n"))
}

fn stage_candidate(
    files: &RepositoryFiles,
    record: &TransactionRecord,
    control: &impl PublicationControl,
) -> Result<()> {
    let scope = files.write_scope(Path::new(".criv"))?;
    scope.write_atomic(
        Path::new(".criv/state-stage/snapshot.json"),
        &record.candidate_state,
    )?;
    control.checkpoint(PublicationStep::StageSnapshot)?;
    scope.write_atomic(
        Path::new(".criv/state-stage/index.json"),
        &record.candidate_index,
    )?;
    control.checkpoint(PublicationStep::StageIndex)?;
    scope.write_atomic(
        Path::new(".criv/state-stage/latest"),
        &format!("{}\n", record.candidate_hash),
    )?;
    control.checkpoint(PublicationStep::StageLatest)
}

fn quarantine_removals(
    files: &RepositoryFiles,
    record: &TransactionRecord,
    control: &impl PublicationControl,
) -> Result<()> {
    let scope = files.write_scope(Path::new(".criv"))?;
    for hash in &record.removals {
        let source = format!(".criv/snapshots/{hash}.json");
        let destination = format!(".criv/state-quarantine/{hash}.json");
        if files.file_exists(Path::new(&source))? && !files.file_exists(Path::new(&destination))? {
            scope.rename_file(Path::new(&source), Path::new(&destination))?;
        }
    }
    control.checkpoint(PublicationStep::Quarantine)
}

fn install_candidate_controlled(
    files: &RepositoryFiles,
    record: &TransactionRecord,
    control: &impl PublicationControl,
) -> Result<()> {
    let snapshots = files.write_scope(Path::new(".criv/snapshots"))?;
    snapshots.write_atomic(
        Path::new(&format!(".criv/snapshots/{}.json", record.candidate_hash)),
        &record.candidate_state,
    )?;
    control.checkpoint(PublicationStep::InstallSnapshot)?;
    snapshots.write_atomic(
        Path::new(".criv/snapshots/index.json"),
        &record.candidate_index,
    )?;
    control.checkpoint(PublicationStep::InstallIndex)?;
    files.write_scope(Path::new(".criv"))?.write_atomic(
        Path::new(".criv/latest"),
        &format!("{}\n", record.candidate_hash),
    )?;
    control.checkpoint(PublicationStep::InstallLatest)
}

fn install_candidate(files: &RepositoryFiles, record: &TransactionRecord) -> Result<()> {
    install_candidate_controlled(files, record, &RealPublicationControl)
}

fn restore_prior(files: &RepositoryFiles, record: &TransactionRecord) -> Result<()> {
    restore_optional(files, ".criv/state.json", record.prior_state.as_deref())?;
    restore_optional(files, ".criv/latest", record.prior_latest.as_deref())?;
    restore_optional(
        files,
        ".criv/snapshots/index.json",
        record.prior_index.as_deref(),
    )?;

    let current = managed_snapshot_paths(files)?;
    let prior = record
        .prior_snapshots
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in current.difference(&prior) {
        remove_optional(files, path)?;
    }
    for (path, contents) in &record.prior_snapshots {
        files
            .write_scope(Path::new(".criv/snapshots"))?
            .write_atomic(Path::new(path), contents)?;
    }
    for hash in &record.removals {
        let quarantined = format!(".criv/state-quarantine/{hash}.json");
        let restored = format!(".criv/snapshots/{hash}.json");
        if files.file_exists(Path::new(&quarantined))?
            && !files.file_exists(Path::new(&restored))?
        {
            files
                .write_scope(Path::new(".criv"))?
                .rename_file(Path::new(&quarantined), Path::new(&restored))?;
        }
    }
    Ok(())
}

fn cleanup_transaction(files: &RepositoryFiles, record: &TransactionRecord) -> Result<()> {
    for hash in &record.removals {
        remove_optional(files, &format!(".criv/state-quarantine/{hash}.json"))?;
    }
    for path in [
        ".criv/state-stage/snapshot.json",
        ".criv/state-stage/index.json",
        ".criv/state-stage/latest",
    ] {
        remove_optional(files, path)?;
    }
    remove_empty_dir(files, STAGE_DIR)?;
    remove_empty_dir(files, QUARANTINE_DIR)?;
    remove_optional(files, TRANSACTION_PATH)
}

fn restore_optional(files: &RepositoryFiles, path: &str, contents: Option<&str>) -> Result<()> {
    match contents {
        Some(contents) => files
            .write_scope(Path::new(".criv"))?
            .write_atomic(Path::new(path), contents),
        None => remove_optional(files, path),
    }
}

fn remove_optional(files: &RepositoryFiles, path: &str) -> Result<()> {
    if files.file_exists(Path::new(path))? {
        files
            .write_scope(Path::new(".criv"))?
            .remove_file(Path::new(path))
    } else {
        Ok(())
    }
}

fn remove_empty_dir(files: &RepositoryFiles, relative: &str) -> Result<()> {
    files
        .write_scope(Path::new(".criv"))?
        .remove_empty_dir(Path::new(relative))
}

fn read_optional(files: &RepositoryFiles, path: &str, label: &str) -> Result<Option<String>> {
    files
        .read_optional_string(Path::new(path))
        .map_err(|error| CrivError::new(format!("failed to read {label} `{path}`: {error}")))
}

fn capture_snapshots(files: &RepositoryFiles) -> Result<BTreeMap<String, String>> {
    let mut snapshots = BTreeMap::new();
    for relative in managed_snapshot_paths(files)? {
        let contents = read_optional(files, &relative, "snapshot")?
            .ok_or_else(|| CrivError::new(format!("snapshot {relative} disappeared")))?;
        snapshots.insert(relative, contents);
    }
    Ok(snapshots)
}

fn managed_snapshot_paths(files: &RepositoryFiles) -> Result<BTreeSet<String>> {
    let Some(names) = files.read_dir_names(Path::new(".criv/snapshots"))? else {
        return Ok(BTreeSet::new());
    };
    let mut paths = BTreeSet::new();
    for name in names {
        let name = name.to_string_lossy().to_string();
        let Some(hash) = name.strip_suffix(".json") else {
            continue;
        };
        if hash.len() == 64 && hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
            paths.insert(format!(".criv/snapshots/{name}"));
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::{Arc, Barrier};

    use super::*;

    struct FailAt {
        step: PublicationStep,
        hits: Cell<usize>,
        also_fail_rollback: bool,
    }

    impl PublicationControl for FailAt {
        fn checkpoint(&self, step: PublicationStep) -> Result<()> {
            if (step == self.step && self.hits.replace(self.hits.get() + 1) == 0)
                || (step == PublicationStep::Rollback && self.also_fail_rollback)
            {
                return Err(CrivError::new(format!("controlled failure at {step:?}")));
            }
            Ok(())
        }
    }

    fn state(seed: &str) -> (String, String) {
        let contents =
            format!("{{\n  \"schema\": \"criv.state.v1\",\n  \"seed\": \"{seed}\"\n}}\n");
        let hash = blake3::hash(contents.trim_end_matches('\n').as_bytes())
            .to_hex()
            .to_string();
        (hash, contents)
    }

    #[test]
    fn each_pre_commit_failure_rolls_back_to_the_prior_revision() {
        for step in [
            PublicationStep::Record,
            PublicationStep::StageSnapshot,
            PublicationStep::StageIndex,
            PublicationStep::StageLatest,
            PublicationStep::Quarantine,
            PublicationStep::InstallSnapshot,
            PublicationStep::InstallIndex,
            PublicationStep::InstallLatest,
            PublicationStep::BeforeCommit,
        ] {
            let root = tempfile::tempdir().unwrap();
            let prior = state("prior");
            let candidate = state("candidate");
            publish(root.path(), &prior.0, &prior.1, 20).unwrap();

            let error = publish_with(
                root.path(),
                &candidate.0,
                &candidate.1,
                20,
                &FailAt {
                    step,
                    hits: Cell::new(0),
                    also_fail_rollback: false,
                },
            )
            .unwrap_err();

            assert!(error.to_string().contains("controlled failure"));
            assert_eq!(
                fs::read_to_string(root.path().join(".criv/state.json")).unwrap(),
                prior.1,
                "failed at {step:?}"
            );
            assert_eq!(
                fs::read_to_string(root.path().join(".criv/latest"))
                    .unwrap()
                    .trim(),
                prior.0,
                "failed at {step:?}"
            );
            assert!(!root.path().join(TRANSACTION_PATH).exists());
        }
    }

    #[test]
    fn interruption_after_commit_recovers_to_the_candidate_revision() {
        let root = tempfile::tempdir().unwrap();
        let prior = state("prior");
        let candidate = state("candidate");
        publish(root.path(), &prior.0, &prior.1, 20).unwrap();

        publish_with(
            root.path(),
            &candidate.0,
            &candidate.1,
            20,
            &FailAt {
                step: PublicationStep::Commit,
                hits: Cell::new(0),
                also_fail_rollback: false,
            },
        )
        .unwrap();
        assert!(root.path().join(TRANSACTION_PATH).exists());

        let loaded = load_snapshot(root.path(), "latest").unwrap().unwrap();
        assert_eq!(loaded, candidate.1);
        assert_eq!(
            fs::read_to_string(root.path().join(".criv/state.json")).unwrap(),
            candidate.1
        );
        assert!(!root.path().join(TRANSACTION_PATH).exists());
    }

    #[test]
    fn rollback_failure_keeps_the_transaction_for_later_recovery() {
        let root = tempfile::tempdir().unwrap();
        let prior = state("prior");
        let candidate = state("candidate");
        publish(root.path(), &prior.0, &prior.1, 20).unwrap();

        let error = publish_with(
            root.path(),
            &candidate.0,
            &candidate.1,
            20,
            &FailAt {
                step: PublicationStep::InstallLatest,
                hits: Cell::new(0),
                also_fail_rollback: true,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("recovery is required"));
        assert!(root.path().join(TRANSACTION_PATH).exists());

        let files = RepositoryFiles::open(root.path()).unwrap();
        recover_locked(&files).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join(".criv/state.json")).unwrap(),
            prior.1
        );
        assert!(!root.path().join(TRANSACTION_PATH).exists());
    }

    #[test]
    fn first_publication_failure_restores_the_no_state_condition() {
        let root = tempfile::tempdir().unwrap();
        let candidate = state("candidate");
        publish_with(
            root.path(),
            &candidate.0,
            &candidate.1,
            20,
            &FailAt {
                step: PublicationStep::InstallLatest,
                hits: Cell::new(0),
                also_fail_rollback: false,
            },
        )
        .unwrap_err();

        for path in [
            ".criv/state.json",
            ".criv/latest",
            ".criv/snapshots/index.json",
            TRANSACTION_PATH,
        ] {
            assert!(!root.path().join(path).exists(), "unexpected {path}");
        }
    }

    #[test]
    fn concurrent_writers_leave_one_complete_revision() {
        let root = tempfile::tempdir().unwrap();
        let root = Arc::new(root.path().to_path_buf());
        let start = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();
        for seed in ["one", "two"] {
            let root = Arc::clone(&root);
            let start = Arc::clone(&start);
            threads.push(std::thread::spawn(move || {
                let candidate = state(seed);
                start.wait();
                publish(&root, &candidate.0, &candidate.1, 20)
            }));
        }
        for thread in threads {
            thread.join().unwrap().unwrap();
        }

        let state = fs::read_to_string(root.join(".criv/state.json")).unwrap();
        let latest = load_snapshot(&root, "latest").unwrap().unwrap();
        assert_eq!(state, latest);
        assert!(!root.join(TRANSACTION_PATH).exists());
    }

    #[test]
    fn publication_lock_timeout_has_a_stable_error() {
        let root = tempfile::tempdir().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let _held = PublicationLock::acquire(&files).unwrap();
        let candidate = state("candidate");

        let error = publish(root.path(), &candidate.0, &candidate.1, 20).unwrap_err();

        assert_eq!(
            error.to_string(),
            "timed out waiting 2 seconds for the State publication lock"
        );
        assert!(!root.path().join(".criv/state.json").exists());
    }

    #[test]
    fn cleanup_failure_keeps_the_committed_revision_for_recovery() {
        let root = tempfile::tempdir().unwrap();
        let candidate = state("candidate");
        publish_with(
            root.path(),
            &candidate.0,
            &candidate.1,
            20,
            &FailAt {
                step: PublicationStep::Cleanup,
                hits: Cell::new(0),
                also_fail_rollback: false,
            },
        )
        .unwrap();
        assert!(root.path().join(TRANSACTION_PATH).exists());

        assert_eq!(
            load_snapshot(root.path(), "latest").unwrap(),
            Some(candidate.1)
        );
        assert!(!root.path().join(TRANSACTION_PATH).exists());
    }

    #[test]
    fn orphan_quarantine_fails_before_state_publication() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(QUARANTINE_DIR)).unwrap();
        fs::write(
            root.path()
                .join(QUARANTINE_DIR)
                .join(format!("{}.json", "a".repeat(64))),
            "do not delete\n",
        )
        .unwrap();
        let candidate = state("candidate");

        let error = publish(root.path(), &candidate.0, &candidate.1, 20).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("exists without a transaction record")
        );
        assert!(!root.path().join(".criv/state.json").exists());
        assert!(root.path().join(QUARANTINE_DIR).exists());
    }

    #[test]
    fn orphan_workspace_file_uses_the_transaction_recovery_error() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".criv")).unwrap();
        fs::write(root.path().join(STAGE_DIR), "stale workspace marker\n").unwrap();
        let candidate = state("candidate");

        let error = publish(root.path(), &candidate.0, &candidate.1, 20).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("exists without a transaction record")
        );
        assert!(!root.path().join(".criv/state.json").exists());
        assert!(root.path().join(STAGE_DIR).is_file());
    }
}

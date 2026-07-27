use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use clap::Args as ClapArgs;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};

use crate::architecture;
use crate::check;
use crate::config::Config;
use crate::source_graph::SourceGraph;
use crate::source_index::{FffSourceIndex, SourceIndex};
use crate::state::{self, State};
use crate::util::create_new_in;
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct WatchOptions {
    #[arg(long)]
    once: bool,
}

pub(crate) fn run(root: &Path, options: WatchOptions) -> Result<()> {
    let _lock = WatchLock::acquire(root)?;
    if options.once {
        run_once(root)?;
        return Ok(());
    }
    // A cold start has no in-process graph to reuse, but the on-disk cache from
    // the previous run is still valid: reuse is keyed on a blake3 content
    // fingerprint, so unchanged files are skipped and changed ones reparsed.
    let cached_graph = crate::source_graph::load_cached(root);
    let config = Config::load(root)?;
    let shared_source_index: Option<Arc<dyn SourceIndex>> = if config.source_index {
        Some(Arc::new(FffSourceIndex::new(
            root,
            &config.source_roots,
            &config.source_exclude,
            true,
        )?))
    } else {
        None
    };
    let (mut vault, mut state) = rebuild(root, cached_graph.as_ref(), shared_source_index.clone())?;
    let mut source_graph = vault.source_graph().clone();

    let docs_path = config.docs_path(root);
    let mut source_watch = shared_source_index
        .as_ref()
        .map(|source_index| {
            source_index
                .source_fingerprint()
                .map(|fingerprint| (source_index.clone(), fingerprint))
        })
        .transpose()?;

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(250), move |event| {
        let _ = tx.send(event);
    })
    .map_err(|err| CrivError::new(format!("failed to start watcher: {err}")))?;

    debouncer
        .watcher()
        .watch(&docs_path, RecursiveMode::Recursive)
        .map_err(|err| CrivError::new(format!("failed to watch docs: {err}")))?;

    println!("criv watch running");

    loop {
        let signal = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(event) => match event {
                Ok(events) if events.is_empty() => WatchSignal::Idle,
                Ok(_) => WatchSignal::DocsChanged,
                Err(err) => {
                    eprintln!("criv watch: watcher error: {err}");
                    WatchSignal::WatcherError
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => WatchSignal::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => WatchSignal::Disconnected,
        };

        let source_changed = if let Some((source_index, source_fingerprint)) = &mut source_watch {
            match source_index.source_fingerprint() {
                Ok(next_fingerprint) if next_fingerprint != *source_fingerprint => {
                    *source_fingerprint = next_fingerprint;
                    true
                }
                Ok(_) => false,
                Err(err) => {
                    eprintln!("criv watch: source index error: {err}");
                    false
                }
            }
        } else {
            false
        };

        match watch_decision(signal, source_changed) {
            WatchDecision::Rebuild { docs_changed } => {
                let previous_graph = (!docs_changed).then_some(&source_graph);
                let previous_state = (!docs_changed).then_some(&state);
                match rebuild_incremental(
                    root,
                    previous_graph,
                    previous_state,
                    shared_source_index.clone(),
                ) {
                    Ok((next_vault, next_state)) => {
                        vault = next_vault;
                        state = next_state;
                        source_graph = vault.source_graph().clone();
                    }
                    Err(err) => eprintln!("criv watch: {err}"),
                }
            }
            WatchDecision::Continue => {}
            WatchDecision::Stop => break,
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WatchSignal {
    DocsChanged,
    Idle,
    WatcherError,
    Disconnected,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WatchDecision {
    Rebuild { docs_changed: bool },
    Continue,
    Stop,
}

fn watch_decision(signal: WatchSignal, source_changed: bool) -> WatchDecision {
    match signal {
        WatchSignal::Disconnected => WatchDecision::Stop,
        WatchSignal::DocsChanged => WatchDecision::Rebuild { docs_changed: true },
        WatchSignal::Idle | WatchSignal::WatcherError if source_changed => WatchDecision::Rebuild {
            docs_changed: false,
        },
        WatchSignal::Idle | WatchSignal::WatcherError => WatchDecision::Continue,
    }
}

/// A single `criv watch --once` rebuild, warmed by the on-disk source graph
/// cache left behind by the previous run.
fn run_once(root: &Path) -> Result<(Vault, State)> {
    let cached_graph = crate::source_graph::load_cached(root);
    rebuild(root, cached_graph.as_ref(), None)
}

fn rebuild(
    root: &Path,
    previous_graph: Option<&SourceGraph>,
    shared_source_index: Option<Arc<dyn SourceIndex>>,
) -> Result<(Vault, State)> {
    let mut vault = previous_graph.map_or_else(
        || Vault::load_incremental_with_source_index(root, None, shared_source_index.clone()),
        |previous_graph| {
            Vault::load_incremental_with_source_index(
                root,
                Some(previous_graph),
                shared_source_index.clone(),
            )
        },
    )?;
    if architecture::write_code_architecture(root, &vault)? {
        vault =
            Vault::load_incremental_with_source_index(root, previous_graph, shared_source_index)?;
    }
    let diagnostics = check::validate_with_previous_state(&vault, None);
    let (snapshot, state) = state::write_state(root, &vault)?;
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();
    println!("state updated: snapshot {snapshot}, {errors} errors, {warnings} warnings");
    Ok((vault, state))
}

fn rebuild_incremental(
    root: &Path,
    previous_graph: Option<&SourceGraph>,
    previous_state: Option<&State>,
    shared_source_index: Option<Arc<dyn SourceIndex>>,
) -> Result<(Vault, State)> {
    let mut vault = Vault::load_incremental_with_source_index(
        root,
        previous_graph,
        shared_source_index.clone(),
    )?;
    if architecture::write_code_architecture(root, &vault)? {
        vault =
            Vault::load_incremental_with_source_index(root, previous_graph, shared_source_index)?;
    }
    let diagnostics = check::validate_with_previous_state(&vault, previous_state);
    let changed_files = previous_state
        .map(|_| vault.source_graph().changed_files())
        .unwrap_or(&[]);
    let (snapshot, state) =
        state::write_state_incremental(root, &vault, previous_state, changed_files)?;
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();
    println!("state updated: snapshot {snapshot}, {errors} errors, {warnings} warnings");
    Ok((vault, state))
}

#[derive(Debug)]
struct WatchLock {
    path: PathBuf,
}

impl WatchLock {
    fn acquire(root: &Path) -> Result<Self> {
        let requested_path = root.join(".criv/watch.lock");
        match Self::try_create(root) {
            Ok(lock) => return Ok(lock),
            Err(CrivError::Io(err)) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(CrivError::new(format!(
                    "failed to acquire watch lock at {}: {err}",
                    requested_path.display()
                )));
            }
        }

        // The lock outlives a crashed or killed watcher, so an existing file is
        // not proof that a watcher is running. Reclaim it when its recorded
        // owner is gone; an unreadable or malformed lock (including one written
        // by an older criv) counts as abandoned rather than wedging the vault.
        let owner = fs::read_to_string(&requested_path)
            .ok()
            .and_then(|contents| LockOwner::parse(&contents));
        if owner.as_ref().is_some_and(LockOwner::is_alive) {
            return Err(CrivError::new(format!(
                "failed to acquire watch lock at {}: an active watcher already owns state refresh; do not start another watch or run `criv watch --once` while it is active",
                requested_path.display()
            )));
        }

        let _ = fs::remove_file(&requested_path);
        Self::try_create(root).map_err(|err| {
            CrivError::new(format!(
                "failed to acquire watch lock at {}: {err}; if no watcher is running, delete that file and retry",
                requested_path.display()
            ))
        })
    }

    fn try_create(root: &Path) -> Result<Self> {
        use std::io::Write;

        let (path, mut file) =
            create_new_in(root, Path::new(".criv"), Path::new(".criv/watch.lock"))?;
        let owner = LockOwner::current();
        file.write_all(owner.serialize().as_bytes())?;
        Ok(Self { path })
    }
}

/// The process recorded as owning a watch lock. The start time distinguishes the
/// original owner from an unrelated process that later reused its PID.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LockOwner {
    pid: u32,
    start: Option<String>,
}

impl LockOwner {
    fn current() -> Self {
        let pid = std::process::id();
        Self {
            start: process_start_time(pid),
            pid,
        }
    }

    fn serialize(&self) -> String {
        format!(
            "pid {}\nstart {}\n",
            self.pid,
            self.start.as_deref().unwrap_or("")
        )
    }

    fn parse(contents: &str) -> Option<Self> {
        let mut pid = None;
        let mut start = None;
        for line in contents.lines() {
            let (key, value) = line.split_once(' ')?;
            match key {
                "pid" => pid = value.trim().parse::<u32>().ok(),
                "start" => {
                    let value = value.trim();
                    start = (!value.is_empty()).then(|| value.to_string());
                }
                _ => return None,
            }
        }
        pid.map(|pid| Self { pid, start })
    }

    fn is_alive(&self) -> bool {
        match process_start_time(self.pid) {
            // The PID is gone, so the owner cannot still be running.
            None => false,
            // A recorded start time that no longer matches means the PID was
            // reused by a different process; the original owner is gone.
            Some(current) => self
                .start
                .as_deref()
                .is_none_or(|recorded| recorded == current),
        }
    }
}

/// Best-effort process start time, which doubles as a liveness probe: `None`
/// means the PID could not be observed as a running process.
///
/// `ps` keeps this dependency-free and works on both macOS and Linux. On any
/// other platform liveness cannot be established, so callers must treat the
/// lock as live rather than reclaiming a lock that may still be held.
fn process_start_time(pid: u32) -> Option<String> {
    if !cfg!(unix) {
        return Some(String::new());
    }
    let output = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let start = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!start.is_empty()).then_some(start)
}

impl Drop for WatchLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn rebuild_includes_generated_code_architecture_in_same_run_state() {
        let temp = TempDir::new().unwrap();
        write_watch_architecture_fixture(temp.path());
        state::reset_work_counts();

        let (vault, _) = rebuild(temp.path(), None, None).unwrap();

        assert_eq!(state::work_counts(), (1, 1));

        assert!(vault.resolve_note("architecture-code").is_some());
        let state: Value = serde_json::from_str(
            &fs::read_to_string(temp.path().join(".criv/state.json")).unwrap(),
        )
        .unwrap();
        let nodes = state["graph"]["nodes"].as_array().unwrap();
        assert!(
            nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("note:architecture-code"))
        );
    }

    #[test]
    fn watch_decisions_distinguish_docs_source_errors_and_shutdown() {
        assert_eq!(
            watch_decision(WatchSignal::DocsChanged, false),
            WatchDecision::Rebuild { docs_changed: true }
        );
        assert_eq!(
            watch_decision(WatchSignal::Idle, true),
            WatchDecision::Rebuild {
                docs_changed: false
            }
        );
        assert_eq!(
            watch_decision(WatchSignal::DocsChanged, true),
            WatchDecision::Rebuild { docs_changed: true }
        );
        assert_eq!(
            watch_decision(WatchSignal::Idle, false),
            WatchDecision::Continue
        );
        assert_eq!(
            watch_decision(WatchSignal::WatcherError, false),
            WatchDecision::Continue
        );
        assert_eq!(
            watch_decision(WatchSignal::WatcherError, true),
            WatchDecision::Rebuild {
                docs_changed: false
            }
        );
        assert_eq!(
            watch_decision(WatchSignal::Disconnected, true),
            WatchDecision::Stop
        );
    }

    #[test]
    fn a_second_watch_once_reuses_the_cached_source_graph() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_watch_architecture_fixture(root);

        let (first, _) = super::run_once(root).unwrap();
        assert_eq!(
            first.source_graph().changed_files(),
            &["src/lib.rs".to_string()],
            "the cold run has nothing to reuse and must parse the file"
        );

        let (second, _) = super::run_once(root).unwrap();

        assert!(
            second.source_graph().changed_files().is_empty(),
            "the warm run must reuse every unchanged file from the on-disk cache"
        );
    }

    #[test]
    fn watch_once_reparses_a_source_file_that_changed_between_runs() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_watch_architecture_fixture(root);

        super::run_once(root).unwrap();
        fs::write(root.join("src/lib.rs"), "fn run() {}\nfn extra() {}\n").unwrap();

        let (second, _) = super::run_once(root).unwrap();

        assert_eq!(
            second.source_graph().changed_files(),
            &["src/lib.rs".to_string()],
            "reuse is keyed on content, so an edited file must still be reparsed"
        );
    }

    #[test]
    fn lock_owner_round_trips_through_the_lock_file_format() {
        let owner = super::LockOwner::current();

        let parsed = super::LockOwner::parse(&owner.serialize()).unwrap();

        assert_eq!(parsed, owner);
        assert!(
            parsed.is_alive(),
            "the running test process must read alive"
        );
    }

    #[test]
    fn lock_owner_rejects_unparseable_contents() {
        for contents in ["active", "pid notanumber\n", "owner 1\n", ""] {
            assert!(
                super::LockOwner::parse(contents).is_none(),
                "{contents:?} must not parse as an owner"
            );
        }
    }

    #[test]
    fn lock_owner_with_a_mismatched_start_time_is_not_alive() {
        // A PID that has been reused by an unrelated process must not be
        // mistaken for the original watcher.
        let owner = super::LockOwner {
            pid: std::process::id(),
            start: Some("Mon Jan  1 00:00:00 2001".to_string()),
        };

        assert!(!owner.is_alive());
    }

    #[test]
    fn acquire_rejects_a_lock_held_by_this_live_process() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(".criv")).unwrap();
        fs::write(
            root.join(".criv/watch.lock"),
            super::LockOwner::current().serialize(),
        )
        .unwrap();

        let error = super::WatchLock::acquire(root).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("active watcher already owns state refresh"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn acquire_reclaims_a_lock_owned_by_a_dead_process() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(".criv")).unwrap();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        fs::write(
            root.join(".criv/watch.lock"),
            format!("pid {dead_pid}\nstart Mon Jan  1 00:00:00 2001\n"),
        )
        .unwrap();

        let lock = super::WatchLock::acquire(root).expect("an abandoned lock must be reclaimable");

        let contents = fs::read_to_string(root.join(".criv/watch.lock")).unwrap();
        assert_eq!(contents, super::LockOwner::current().serialize());
        drop(lock);
        assert!(!root.join(".criv/watch.lock").exists());
    }

    fn write_watch_architecture_fixture(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[architecture.code]
output = "docs/architecture/04-code.md"
title = "Code diagram for criv"
"#,
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
    }
}

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
        rebuild(root, None, None)?;
        return Ok(());
    }
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
    let (mut vault, mut state) = rebuild(root, None, shared_source_index.clone())?;
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

struct WatchLock {
    path: PathBuf,
}

impl WatchLock {
    fn acquire(root: &Path) -> Result<Self> {
        let requested_path = root.join(".criv/watch.lock");
        let (path, _) = match create_new_in(root, Path::new(".criv"), Path::new(".criv/watch.lock"))
        {
            Ok(lock) => lock,
            Err(CrivError::Io(err)) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(CrivError::new(format!(
                    "failed to acquire watch lock at {}: an active watcher already owns state refresh; do not start another watch or run `criv watch --once` while it is active",
                    requested_path.display()
                )));
            }
            Err(err) => {
                return Err(CrivError::new(format!(
                    "failed to acquire watch lock at {}: {err}",
                    requested_path.display()
                )));
            }
        };
        Ok(Self { path })
    }
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

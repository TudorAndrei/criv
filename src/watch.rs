use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clap::Args as ClapArgs;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};

use crate::check;
use crate::config::Config;
use crate::source_graph::SourceGraph;
use crate::source_index::{FffSourceIndex, SourceIndex};
use crate::state::{self, State};
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct WatchOptions {
    #[arg(long)]
    once: bool,
}

pub(crate) fn run(root: &Path, options: WatchOptions) -> Result<()> {
    let mut vault = rebuild(root, None)?;
    if options.once {
        return Ok(());
    }
    let mut source_graph = vault.source_graph().clone();
    let mut state = State::build(root, &vault)?;
    let _lock = WatchLock::acquire(root)?;

    let config = Config::load(root)?;
    let docs_path = config.docs_path(root);
    let mut source_watch = if config.source_index {
        let source_index =
            FffSourceIndex::new(root, &config.source_roots, &config.source_exclude, true)?;
        let source_fingerprint = source_index.source_fingerprint()?;
        Some((source_index, source_fingerprint))
    } else {
        None
    };

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
        let mut docs_changed = false;
        let mut source_changed = false;
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(event) => match event {
                Ok(events) if events.is_empty() => {}
                Ok(_) => docs_changed = true,
                Err(err) => eprintln!("criv watch: watcher error: {err}"),
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some((source_index, source_fingerprint)) = &mut source_watch {
            match source_index.source_fingerprint() {
                Ok(next_fingerprint) if next_fingerprint != *source_fingerprint => {
                    *source_fingerprint = next_fingerprint;
                    source_changed = true;
                }
                Ok(_) => {}
                Err(err) => eprintln!("criv watch: source index error: {err}"),
            }
        }

        if docs_changed || source_changed {
            let previous_graph = (!docs_changed).then_some(&source_graph);
            let previous_state = (!docs_changed).then_some(&state);
            match rebuild_incremental(root, previous_graph, previous_state) {
                Ok((next_vault, next_state)) => {
                    vault = next_vault;
                    state = next_state;
                    source_graph = vault.source_graph().clone();
                }
                Err(err) => eprintln!("criv watch: {err}"),
            }
        }
    }

    Ok(())
}

fn rebuild(root: &Path, previous_graph: Option<&SourceGraph>) -> Result<Vault> {
    let vault = Vault::load_incremental(root, previous_graph)?;
    let diagnostics = check::validate(&vault);
    let snapshot = state::write_state(root, &vault)?;
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();
    println!("state updated: snapshot {snapshot}, {errors} errors, {warnings} warnings");
    Ok(vault)
}

fn rebuild_incremental(
    root: &Path,
    previous_graph: Option<&SourceGraph>,
    previous_state: Option<&State>,
) -> Result<(Vault, State)> {
    let vault = Vault::load_incremental(root, previous_graph)?;
    let diagnostics = check::validate(&vault);
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
        let criv_dir = root.join(".criv");
        fs::create_dir_all(&criv_dir)?;
        let path = criv_dir.join("watch.lock");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|err| {
                CrivError::new(format!(
                    "failed to acquire watch lock at {}: {err}",
                    path.display()
                ))
            })?;
        Ok(Self { path })
    }
}

impl Drop for WatchLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

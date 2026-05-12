use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clap::Args as ClapArgs;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};

use crate::check;
use crate::state;
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct WatchOptions {
    #[arg(long)]
    once: bool,
    #[arg(long)]
    port: Option<u16>,
}

pub(crate) fn run(root: &Path, options: WatchOptions) -> Result<()> {
    rebuild(root)?;
    if options.once {
        return Ok(());
    }
    let _lock = WatchLock::acquire(root)?;

    let vault = Vault::load(root)?;
    let docs_path = vault.config.docs_path(root);
    let source_roots = vault.config.source_root_paths(root);

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(250), move |event| {
        let _ = tx.send(event);
    })
    .map_err(|err| CrivError::new(format!("failed to start watcher: {err}")))?;

    debouncer
        .watcher()
        .watch(&docs_path, RecursiveMode::Recursive)
        .map_err(|err| CrivError::new(format!("failed to watch docs: {err}")))?;
    for source_root in source_roots {
        debouncer
            .watcher()
            .watch(&source_root, RecursiveMode::Recursive)
            .map_err(|err| CrivError::new(format!("failed to watch source root: {err}")))?;
    }

    if let Some(port) = options.port {
        println!(
            "criv watch running; status port {port} is reserved but no endpoint is exposed yet"
        );
    } else {
        println!("criv watch running");
    }

    for event in rx {
        match event {
            Ok(events) if events.is_empty() => {}
            Ok(_) => {
                if let Err(err) = rebuild(root) {
                    eprintln!("criv watch: {err}");
                }
            }
            Err(err) => eprintln!("criv watch: watcher error: {err}"),
        }
    }

    Ok(())
}

fn rebuild(root: &Path) -> Result<()> {
    let vault = Vault::load(root)?;
    let diagnostics = check::validate(&vault);
    let snapshot = state::write_state(root, &vault)?;
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();
    println!("state updated: snapshot {snapshot}, {errors} errors, {warnings} warnings");
    Ok(())
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

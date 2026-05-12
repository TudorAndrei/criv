use std::path::Path;
use std::sync::mpsc;

use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::check;
use crate::state;
use crate::vault::Vault;
use crate::{Args, CrivError, Result};

#[derive(Debug, Default)]
pub(crate) struct WatchOptions {
    once: bool,
    port: Option<u16>,
}

impl WatchOptions {
    pub(crate) fn parse(mut args: Args) -> Result<Self> {
        let mut options = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--once" => options.once = true,
                "--port" => {
                    let value = args.expect_value("--port")?;
                    options.port = Some(
                        value
                            .parse()
                            .map_err(|_| CrivError::usage(format!("invalid port `{value}`")))?,
                    );
                }
                other => return Err(CrivError::usage(format!("unknown watch option `{other}`"))),
            }
        }
        Ok(options)
    }
}

pub(crate) fn run(root: &Path, options: WatchOptions) -> Result<()> {
    rebuild(root)?;
    if options.once {
        return Ok(());
    }

    let vault = Vault::load(root)?;
    let docs_path = vault.config.docs_path(root);
    let source_roots = vault.config.source_root_paths(root);

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = tx.send(event);
        },
        NotifyConfig::default(),
    )
    .map_err(|err| CrivError::new(format!("failed to start watcher: {err}")))?;

    watcher
        .watch(&docs_path, RecursiveMode::Recursive)
        .map_err(|err| CrivError::new(format!("failed to watch docs: {err}")))?;
    for source_root in source_roots {
        watcher
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
            Ok(event) if event.kind.is_access() => {}
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
    state::write_state(root, &vault)?;
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();
    println!("state updated: {errors} errors, {warnings} warnings");
    Ok(())
}

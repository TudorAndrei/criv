use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clap::Args as ClapArgs;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};

use crate::config::Config;
use crate::refresh::{RefreshCause, RefreshSession};
use crate::source_index::SourceChange;
use crate::util::create_new_in;
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
    let config = Config::load(root)?;
    let mut refresh = RefreshSession::live(root, &config)?;
    refresh.refresh(root, RefreshCause::Initial)?;

    let docs_path = config.docs_path(root);

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

        let source_changed = match refresh.observe_source_change() {
            Ok(SourceChange::Changed) => true,
            Ok(SourceChange::Unchanged | SourceChange::Disabled) => false,
            Err(err) => {
                eprintln!("criv watch: source index error: {err}");
                false
            }
        };

        match watch_decision(signal, source_changed) {
            WatchDecision::Rebuild { docs_changed } => {
                let cause = if docs_changed {
                    RefreshCause::DocsChanged
                } else {
                    RefreshCause::SourceChanged
                };
                match refresh.refresh(root, cause) {
                    Ok(_) => {}
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
fn run_once(root: &Path) -> Result<()> {
    let mut refresh = RefreshSession::one_shot(root)?;
    refresh.refresh(root, RefreshCause::Initial)?;
    Ok(())
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
        let mut contents = format!("pid {}\n", self.pid);
        if let Some(start) = &self.start {
            contents.push_str(&format!("start {start}\n"));
        }
        contents
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
                    // Older Unix locks used an empty field for a missing
                    // timestamp. On Windows it is the platform's explicit
                    // unknown-start sentinel and must round-trip instead.
                    start = if value.is_empty() && cfg!(unix) {
                        None
                    } else {
                        Some(value.to_string())
                    };
                }
                _ => return None,
            }
        }
        pid.map(|pid| Self { pid, start })
    }

    fn is_alive(&self) -> bool {
        // A process cannot reuse its own PID. This also avoids depending on
        // platform process inspection for the current watcher.
        if self.pid == std::process::id() && self.start.is_none() {
            return true;
        }
        match process_start_time(self.pid) {
            // If start-time inspection is unavailable, fall back to a PID
            // probe. This is deliberately conservative: keeping a lock held
            // is safer than reclaiming one from a live watcher.
            None => process_is_alive(self.pid),
            // A recorded start time that no longer matches means the PID was
            // reused by a different process; the original owner is gone.
            Some(current) => self
                .start
                .as_deref()
                .is_none_or(|recorded| recorded == current),
        }
    }
}

fn process_is_alive(pid: u32) -> bool {
    if !cfg!(unix) {
        return true;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
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
    use tempfile::TempDir;

    use super::*;

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
    fn lock_owner_round_trips_an_unknown_start_time() {
        let owner = super::LockOwner {
            pid: 42,
            start: Some(String::new()),
        };

        let expected = if cfg!(unix) {
            super::LockOwner {
                pid: owner.pid,
                start: None,
            }
        } else {
            owner.clone()
        };
        assert_eq!(super::LockOwner::parse(&owner.serialize()), Some(expected));
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
        if super::process_start_time(std::process::id()).is_none() {
            // The fallback deliberately treats an uninspectable live PID as
            // alive, so it cannot prove PID reuse in this environment.
            return;
        }
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
}

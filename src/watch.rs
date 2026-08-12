use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use clap::Args as ClapArgs;
use notify_debouncer_mini::{
    DebounceEventResult, Debouncer, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};

use crate::config::Config;
use crate::refresh::{RefreshCause, RefreshSession};
use crate::source_index::SourceChange;
use crate::util::open_regular_file_in;
use crate::{CrivError, Result};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct WatchOptions {
    #[arg(long)]
    once: bool,
}

pub(crate) fn run(root: &Path, options: WatchOptions) -> Result<()> {
    let mode = if options.once {
        WatchMode::Once
    } else {
        WatchMode::Live
    };
    let _lock = WatchSessionLock::acquire(root, mode)?;
    if options.once {
        run_once(root)?;
        return Ok(());
    }
    let (mut _debouncer, mut rx) = start_repository_watcher(root)?;
    let mut session = LiveWatchSession::start(root)?;

    println!("criv watch running");

    loop {
        let signal = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(event) => match event {
                Ok(events) if events.is_empty() => WatchSignal::Idle,
                Ok(events) => {
                    WatchSignal::Paths(events.into_iter().map(|event| event.path).collect())
                }
                Err(err) => {
                    eprintln!("criv watch: watcher error: {err}");
                    session.suspend("watcher adapter error");
                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                        match start_repository_watcher(root) {
                            Ok((replacement, replacement_rx)) => {
                                _debouncer = replacement;
                                rx = replacement_rx;
                                session.reconfigure(root);
                                break;
                            }
                            Err(restart_error) => {
                                session.suspend(&format!("watcher adapter error: {restart_error}"));
                            }
                        }
                    }
                    continue;
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => WatchSignal::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CrivError::new("watcher event channel disconnected"));
            }
        };

        if session.must_reconfigure(root, &signal) {
            session.reconfigure(root);
            continue;
        }

        let source_changed = match session.refresh.observe_source_change() {
            Ok(SourceChange::Changed) => true,
            Ok(SourceChange::Unchanged | SourceChange::Disabled) => false,
            Err(err) => {
                eprintln!("criv watch: source index error: {err}");
                session.suspend(&format!("source index error: {err}"));
                false
            }
        };
        if session.suspended {
            continue;
        }

        let docs_changed = session.docs_changed(&signal);
        match watch_decision(docs_changed, source_changed) {
            WatchDecision::Rebuild { docs_changed } => {
                let cause = if docs_changed {
                    RefreshCause::DocsChanged
                } else {
                    RefreshCause::SourceChanged
                };
                match session.refresh.refresh(root, cause) {
                    Ok(_) => {}
                    Err(err) => eprintln!("criv watch: {err}"),
                }
            }
            WatchDecision::Continue => {}
        }
    }
}

type RepositoryDebouncer = Debouncer<RecommendedWatcher>;
type WatchEventReceiver = mpsc::Receiver<DebounceEventResult>;

fn start_repository_watcher(root: &Path) -> Result<(RepositoryDebouncer, WatchEventReceiver)> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(250), move |event| {
        let _ = tx.send(event);
    })
    .map_err(|err| CrivError::new(format!("failed to start watcher: {err}")))?;
    debouncer
        .watcher()
        .watch(root, RecursiveMode::Recursive)
        .map_err(|err| CrivError::new(format!("failed to watch repository: {err}")))?;
    Ok((debouncer, rx))
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum WatchSignal {
    Paths(Vec<PathBuf>),
    Idle,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WatchDecision {
    Rebuild { docs_changed: bool },
    Continue,
}

fn watch_decision(docs_changed: bool, source_changed: bool) -> WatchDecision {
    match (docs_changed, source_changed) {
        (true, _) => WatchDecision::Rebuild { docs_changed: true },
        (false, true) => WatchDecision::Rebuild {
            docs_changed: false,
        },
        (false, false) => WatchDecision::Continue,
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WatchTopology {
    paths: Vec<(String, PathKind)>,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum PathKind {
    Missing,
    File,
    Directory,
    Unsafe,
}

#[derive(Debug)]
struct LiveWatchSession {
    config: Config,
    config_source: Option<String>,
    refresh: RefreshSession,
    docs_path: PathBuf,
    topology: WatchTopology,
    suspended: bool,
    failure: Option<String>,
    next_retry: Instant,
}

impl LiveWatchSession {
    fn start(root: &Path) -> Result<Self> {
        let config_source = read_config_source(root)?;
        let config = Config::parse(config_source.as_deref())?;
        Self::candidate(root, config, config_source)
    }

    fn candidate(root: &Path, config: Config, config_source: Option<String>) -> Result<Self> {
        let docs_path = config.docs_path(root);
        require_real_docs_root(&docs_path)?;
        let topology = WatchTopology::observe(root, &config);
        let mut refresh = RefreshSession::live(root, &config)?;
        refresh.refresh(root, RefreshCause::Initial)?;
        Ok(Self {
            config,
            config_source,
            refresh,
            docs_path,
            topology,
            suspended: false,
            failure: None,
            next_retry: Instant::now(),
        })
    }

    fn docs_changed(&self, signal: &WatchSignal) -> bool {
        matches!(signal, WatchSignal::Paths(paths) if paths.iter().any(|path| path.starts_with(&self.docs_path)))
    }

    fn must_reconfigure(&self, root: &Path, signal: &WatchSignal) -> bool {
        if self.suspended {
            return !matches!(signal, WatchSignal::Idle) || Instant::now() >= self.next_retry;
        }
        let config_changed = match read_config_source(root) {
            Ok(source) => source != self.config_source,
            Err(_) => true,
        };
        matches!(signal, WatchSignal::Paths(_))
            && (config_changed || WatchTopology::observe(root, &self.config) != self.topology)
    }

    fn reconfigure(&mut self, root: &Path) {
        let candidate = read_config_source(root).and_then(|config_source| {
            Config::parse(config_source.as_deref())
                .and_then(|config| Self::candidate(root, config, config_source))
        });
        match candidate {
            Ok(mut candidate) => {
                if self.suspended {
                    eprintln!("criv watch: reconfiguration recovered");
                }
                candidate.failure = None;
                *self = candidate;
            }
            Err(err) => self.suspend(&err.to_string()),
        }
    }

    fn suspend(&mut self, cause: &str) {
        self.suspended = true;
        self.next_retry = Instant::now() + Duration::from_secs(1);
        if self.failure.as_deref() != Some(cause) {
            eprintln!("criv watch: reconfiguration failed: {cause}; keeping last successful State");
            self.failure = Some(cause.to_string());
        }
    }
}

fn read_config_source(root: &Path) -> Result<Option<String>> {
    let path = root.join("criv.toml");
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl WatchTopology {
    fn observe(root: &Path, config: &Config) -> Self {
        let mut paths = config
            .source_roots
            .iter()
            .map(|path| (path.clone(), path_kind(&root.join(path))))
            .collect::<Vec<_>>();
        paths.push((config.docs_dir.clone(), path_kind(&config.docs_path(root))));
        paths.sort();
        Self { paths }
    }
}

fn path_kind(path: &Path) -> PathKind {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => PathKind::Unsafe,
        Ok(metadata) if metadata.is_file() => PathKind::File,
        Ok(metadata) if metadata.is_dir() => PathKind::Directory,
        Ok(_) => PathKind::Unsafe,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PathKind::Missing,
        Err(_) => PathKind::Unsafe,
    }
}

fn require_real_docs_root(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => Ok(()),
        Ok(_) => Err(CrivError::new(format!(
            "configured docs root {} must be a real directory",
            path.display()
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(CrivError::new(format!(
            "configured docs root {} does not exist",
            path.display()
        ))),
        Err(err) => Err(err.into()),
    }
}

/// A single `criv watch --once` rebuild, warmed by the on-disk source graph
/// cache left behind by the previous run.
fn run_once(root: &Path) -> Result<()> {
    let mut refresh = RefreshSession::one_shot(root)?;
    refresh.refresh(root, RefreshCause::Initial)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum WatchMode {
    Live,
    Once,
}

impl WatchMode {
    fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Once => "once",
        }
    }
}

#[derive(Debug)]
struct WatchSessionLock {
    _file: fs::File,
}

impl WatchSessionLock {
    fn acquire(root: &Path, mode: WatchMode) -> Result<Self> {
        let requested_path = root.join(".criv/watch.lock");
        let (_, mut file) =
            open_regular_file_in(root, Path::new(".criv"), Path::new(".criv/watch.lock")).map_err(
                |err| {
                    CrivError::new(format!(
                        "unsafe watch lock path {}: {err}",
                        requested_path.display()
                    ))
                },
            )?;

        if let Err(err) = file.try_lock() {
            if matches!(err, fs::TryLockError::WouldBlock) {
                let detail = read_watch_lock_record(&mut file)
                    .map(|record| format!(" (mode {}, pid {})", record.mode, record.pid))
                    .unwrap_or_default();
                return Err(CrivError::new(format!(
                    "another watch session owns State refresh{detail}; do not start another watch or run `criv watch --once` while it is active"
                )));
            }
            return Err(CrivError::new(format!(
                "failed to acquire operating-system watch lock at {}: {err}",
                requested_path.display()
            )));
        }

        let record = format!(
            "schema criv.watch-lock.v1\npid {}\nmode {}\n",
            std::process::id(),
            mode.label()
        );
        file.set_len(0)
            .and_then(|_| file.rewind())
            .and_then(|_| file.write_all(record.as_bytes()))
            .and_then(|_| file.sync_all())
            .map_err(|err| {
                CrivError::new(format!(
                    "failed to publish watch lock diagnostics at {}: {err}",
                    requested_path.display()
                ))
            })?;
        Ok(Self { _file: file })
    }
}

struct WatchLockRecord {
    pid: u32,
    mode: &'static str,
}

fn read_watch_lock_record(file: &mut fs::File) -> Option<WatchLockRecord> {
    file.rewind().ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let lines = contents.lines().collect::<Vec<_>>();
    if lines.len() != 3 || lines[0] != "schema criv.watch-lock.v1" {
        return None;
    }
    let pid = lines[1].strip_prefix("pid ")?.parse().ok()?;
    let mode = match lines[2].strip_prefix("mode ")? {
        "live" => "live",
        "once" => "once",
        _ => return None,
    };
    Some(WatchLockRecord { pid, mode })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn watch_decisions_distinguish_docs_and_source_changes() {
        assert_eq!(
            watch_decision(true, false),
            WatchDecision::Rebuild { docs_changed: true }
        );
        assert_eq!(
            watch_decision(false, true),
            WatchDecision::Rebuild {
                docs_changed: false
            }
        );
        assert_eq!(
            watch_decision(true, true),
            WatchDecision::Rebuild { docs_changed: true }
        );
        assert_eq!(watch_decision(false, false), WatchDecision::Continue);
    }

    #[test]
    fn operating_system_lock_allows_exactly_one_owner_and_safe_handoff() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(".criv")).unwrap();
        let first = super::WatchSessionLock::acquire(root, super::WatchMode::Live).unwrap();

        let error = super::WatchSessionLock::acquire(root, super::WatchMode::Once).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("another watch session owns State refresh"),
            "unexpected error: {error}"
        );
        drop(first);
        assert!(root.join(".criv/watch.lock").is_file());

        super::WatchSessionLock::acquire(root, super::WatchMode::Once).unwrap();
    }

    #[test]
    fn old_or_malformed_metadata_never_controls_ownership() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(".criv")).unwrap();
        fs::write(root.join(".criv/watch.lock"), "not a lock record\n").unwrap();

        let lock = super::WatchSessionLock::acquire(root, super::WatchMode::Live).unwrap();

        let contents = fs::read_to_string(root.join(".criv/watch.lock")).unwrap();
        assert_eq!(
            contents,
            format!(
                "schema criv.watch-lock.v1\npid {}\nmode live\n",
                std::process::id()
            )
        );
        drop(lock);
    }

    #[test]
    fn many_simultaneous_contenders_produce_one_owner() {
        const CONTENDERS: usize = 16;
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join(".criv")).unwrap();
        let root = Arc::new(temp.path().to_path_buf());
        let start = Arc::new(Barrier::new(CONTENDERS));
        let attempts = Arc::new(AtomicUsize::new(0));
        let owners = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..CONTENDERS {
            let root = Arc::clone(&root);
            let start = Arc::clone(&start);
            let attempts = Arc::clone(&attempts);
            let owners = Arc::clone(&owners);
            threads.push(std::thread::spawn(move || {
                start.wait();
                let lock = WatchSessionLock::acquire(&root, WatchMode::Live);
                attempts.fetch_add(1, Ordering::SeqCst);
                if let Ok(lock) = lock {
                    owners.fetch_add(1, Ordering::SeqCst);
                    while attempts.load(Ordering::SeqCst) < CONTENDERS {
                        std::thread::yield_now();
                    }
                    drop(lock);
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(owners.load(Ordering::SeqCst), 1);
    }
}

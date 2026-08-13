use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use clap::Args as ClapArgs;
use notify_debouncer_mini::{
    DebounceEventResult, Debouncer, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};

use crate::config::Config;
use crate::refresh::{RefreshCause, RefreshSession};
use crate::source::SourceChange;
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
    let mut session = LiveWatchSession::start(root)?;

    println!("criv watch running");

    loop {
        session.step(Duration::from_millis(250))?;
    }
}

type RepositoryDebouncer = Debouncer<RecommendedWatcher>;
type WatchEventReceiver = mpsc::Receiver<DebounceEventResult>;

#[derive(Debug, Clone, Eq, PartialEq)]
enum WatcherPoll {
    Paths(Vec<PathBuf>),
    Idle,
    Error(String),
    Disconnected,
}

trait WatcherAdapter: std::fmt::Debug {
    fn poll(&mut self, timeout: Duration) -> WatcherPoll;
}

trait WatcherFactory: std::fmt::Debug {
    fn start(&self, set: &WatchSet) -> Result<Box<dyn WatcherAdapter>>;
}

#[derive(Debug)]
struct NotifyWatcherFactory;

impl WatcherFactory for NotifyWatcherFactory {
    fn start(&self, set: &WatchSet) -> Result<Box<dyn WatcherAdapter>> {
        Ok(Box::new(NotifyWatcherAdapter::start(set)?))
    }
}

#[derive(Debug)]
struct NotifyWatcherAdapter {
    _debouncer: RepositoryDebouncer,
    receiver: WatchEventReceiver,
}

impl NotifyWatcherAdapter {
    fn start(set: &WatchSet) -> Result<Self> {
        let (tx, receiver) = mpsc::channel::<DebounceEventResult>();
        let mut debouncer = new_debouncer(Duration::from_millis(250), move |event| {
            let _ = tx.send(event);
        })
        .map_err(|err| CrivError::new(format!("failed to start watcher: {err}")))?;
        for target in &set.targets {
            let mode = match target.depth {
                WatchDepth::NonRecursive => RecursiveMode::NonRecursive,
                WatchDepth::Recursive => RecursiveMode::Recursive,
            };
            debouncer
                .watcher()
                .watch(&target.path, mode)
                .map_err(|err| {
                    CrivError::new(format!("failed to watch {}: {err}", target.path.display()))
                })?;
        }
        Ok(Self {
            _debouncer: debouncer,
            receiver,
        })
    }
}

impl WatcherAdapter for NotifyWatcherAdapter {
    fn poll(&mut self, timeout: Duration) -> WatcherPoll {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(events)) if events.is_empty() => WatcherPoll::Idle,
            Ok(Ok(events)) => {
                WatcherPoll::Paths(events.into_iter().map(|event| event.path).collect())
            }
            Ok(Err(err)) => WatcherPoll::Error(err.to_string()),
            Err(mpsc::RecvTimeoutError::Timeout) => WatcherPoll::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => WatcherPoll::Disconnected,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WatchSet {
    targets: Vec<WatchTarget>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WatchTarget {
    path: PathBuf,
    depth: WatchDepth,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WatchDepth {
    NonRecursive,
    Recursive,
}

impl WatchTarget {
    fn non_recursive(path: PathBuf) -> Self {
        Self {
            path,
            depth: WatchDepth::NonRecursive,
        }
    }

    fn recursive(path: PathBuf) -> Self {
        Self {
            path,
            depth: WatchDepth::Recursive,
        }
    }
}

impl WatchSet {
    fn active(root: &Path, config: &Config) -> Self {
        let mut targets = BTreeMap::new();
        insert_watch_target(&mut targets, root.to_path_buf(), WatchDepth::NonRecursive);
        insert_existing_or_ancestor(&mut targets, root, &config.docs_path(root));
        for source_root in &config.source_roots {
            insert_existing_or_ancestor(&mut targets, root, &root.join(source_root));
        }
        Self {
            targets: targets
                .into_iter()
                .map(|(path, depth)| match depth {
                    WatchDepth::NonRecursive => WatchTarget::non_recursive(path),
                    WatchDepth::Recursive => WatchTarget::recursive(path),
                })
                .collect(),
        }
    }

    fn recovery(root: &Path, config: Option<&Config>) -> Self {
        let mut targets = BTreeMap::new();
        insert_watch_target(&mut targets, root.to_path_buf(), WatchDepth::NonRecursive);
        if let Some(config) = config {
            insert_existing_or_ancestor(&mut targets, root, &config.docs_path(root));
            for source_root in &config.source_roots {
                let path = root.join(source_root);
                if matches!(path_kind(&path), PathKind::Missing | PathKind::Unsafe) {
                    insert_watch_target(
                        &mut targets,
                        nearest_existing_directory(root, &path),
                        WatchDepth::NonRecursive,
                    );
                }
            }
        }
        Self {
            targets: targets
                .into_iter()
                .map(|(path, depth)| match depth {
                    WatchDepth::NonRecursive => WatchTarget::non_recursive(path),
                    WatchDepth::Recursive => WatchTarget::recursive(path),
                })
                .collect(),
        }
    }
}

fn insert_existing_or_ancestor(
    targets: &mut BTreeMap<PathBuf, WatchDepth>,
    root: &Path,
    requested: &Path,
) {
    match path_kind(requested) {
        PathKind::Directory => {
            insert_watch_target(targets, requested.to_path_buf(), WatchDepth::Recursive);
        }
        PathKind::File => {
            insert_watch_target(targets, requested.to_path_buf(), WatchDepth::NonRecursive);
        }
        PathKind::Missing | PathKind::Unsafe => {
            insert_watch_target(
                targets,
                nearest_existing_directory(root, requested),
                WatchDepth::NonRecursive,
            );
        }
    }
}

fn insert_watch_target(
    targets: &mut BTreeMap<PathBuf, WatchDepth>,
    path: PathBuf,
    depth: WatchDepth,
) {
    targets
        .entry(path)
        .and_modify(|current| {
            if depth == WatchDepth::Recursive {
                *current = WatchDepth::Recursive;
            }
        })
        .or_insert(depth);
}

fn nearest_existing_directory(root: &Path, requested: &Path) -> PathBuf {
    let mut candidate = requested.to_path_buf();
    loop {
        if candidate.starts_with(root) && path_kind(&candidate) == PathKind::Directory {
            return candidate;
        }
        if candidate == root || !candidate.pop() {
            return root.to_path_buf();
        }
    }
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
struct WatchBinding {
    set: WatchSet,
    adapter: Box<dyn WatcherAdapter>,
}

impl WatchBinding {
    fn start(factory: &dyn WatcherFactory, set: WatchSet) -> Result<Self> {
        let adapter = factory.start(&set)?;
        Ok(Self { set, adapter })
    }
}

#[derive(Debug)]
struct ActiveWatchGeneration {
    config: Config,
    config_source: Option<String>,
    refresh: RefreshSession,
    docs_path: PathBuf,
    topology: WatchTopology,
    watcher: WatchBinding,
}

#[derive(Debug)]
struct CandidateFailure {
    error: CrivError,
    watcher_unavailable: bool,
}

impl CandidateFailure {
    fn candidate(error: CrivError) -> Self {
        Self {
            error,
            watcher_unavailable: false,
        }
    }

    fn watcher(error: CrivError) -> Self {
        Self {
            error,
            watcher_unavailable: true,
        }
    }
}

impl From<CrivError> for CandidateFailure {
    fn from(error: CrivError) -> Self {
        Self::candidate(error)
    }
}

impl ActiveWatchGeneration {
    fn candidate(
        root: &Path,
        config: Config,
        config_source: Option<String>,
        factory: &dyn WatcherFactory,
    ) -> std::result::Result<Self, CandidateFailure> {
        let docs_path = config.docs_path(root);
        require_real_docs_root(&docs_path).map_err(CandidateFailure::candidate)?;
        let topology = WatchTopology::observe(root, &config);
        let watcher = WatchBinding::start(factory, WatchSet::active(root, &config))
            .map_err(CandidateFailure::watcher)?;
        let mut refresh =
            RefreshSession::live(root, &config).map_err(CandidateFailure::candidate)?;
        refresh
            .refresh(root, RefreshCause::Initial)
            .map_err(CandidateFailure::candidate)?;
        Ok(Self {
            config,
            config_source,
            refresh,
            docs_path,
            topology,
            watcher,
        })
    }
}

#[derive(Debug)]
struct LiveWatchSession {
    root: PathBuf,
    active: ActiveWatchGeneration,
    recovery: Option<WatchBinding>,
    watcher_factory: Arc<dyn WatcherFactory>,
    suspended: bool,
    failure: Option<String>,
    next_retry: Instant,
}

impl LiveWatchSession {
    fn start(root: &Path) -> Result<Self> {
        Self::start_with_factory(root, Arc::new(NotifyWatcherFactory))
    }

    fn start_with_factory(root: &Path, watcher_factory: Arc<dyn WatcherFactory>) -> Result<Self> {
        let config_source = read_config_source(root)?;
        let config = Config::parse(config_source.as_deref())?;
        let active =
            ActiveWatchGeneration::candidate(root, config, config_source, watcher_factory.as_ref())
                .map_err(|failure| failure.error)?;
        Ok(Self {
            root: root.to_path_buf(),
            active,
            recovery: None,
            watcher_factory,
            suspended: false,
            failure: None,
            next_retry: Instant::now(),
        })
    }

    fn step(&mut self, timeout: Duration) -> Result<()> {
        let signal = self.poll(timeout)?;
        if self.must_reconfigure(&signal) {
            self.reconfigure();
            return Ok(());
        }
        if self.suspended {
            return Ok(());
        }

        let source_changed = match self.active.refresh.observe_source_change() {
            Ok(SourceChange::Changed) => true,
            Ok(SourceChange::Unchanged | SourceChange::Disabled) => false,
            Err(err) => {
                eprintln!("criv watch: source index error: {err}");
                self.recovery = None;
                self.suspend(&format!("source index error: {err}"));
                return Ok(());
            }
        };
        if self.must_reconfigure(&signal) {
            self.reconfigure();
            return Ok(());
        }
        let docs_changed = self.docs_changed(&signal);
        if let WatchDecision::Rebuild { docs_changed } =
            watch_decision(docs_changed, source_changed)
        {
            let cause = if docs_changed {
                RefreshCause::DocsChanged
            } else {
                RefreshCause::SourceChanged
            };
            let expected_config_source = self.active.config_source.clone();
            let root = self.root.clone();
            let result = self
                .active
                .refresh
                .refresh_with_precommit_check(&root, cause, || match read_config_source(&root) {
                    Ok(source) if source == expected_config_source => Ok(()),
                    Ok(_) => Err(CrivError::new(
                        "watch configuration changed before State publication",
                    )),
                    Err(err) => Err(err),
                });
            if read_config_source(&self.root).ok() != Some(self.active.config_source.clone()) {
                self.reconfigure();
                return Ok(());
            }
            if let Err(err) = result {
                eprintln!("criv watch: {err}");
            }
        }
        Ok(())
    }

    fn poll(&mut self, timeout: Duration) -> Result<WatchSignal> {
        let poll = if self.suspended {
            match self.recovery.as_mut() {
                Some(recovery) => recovery.adapter.poll(timeout),
                None => {
                    if !timeout.is_zero() {
                        std::thread::sleep(timeout);
                    }
                    WatcherPoll::Idle
                }
            }
        } else {
            self.active.watcher.adapter.poll(timeout)
        };
        match poll {
            WatcherPoll::Paths(paths) if paths.is_empty() => Ok(WatchSignal::Idle),
            WatcherPoll::Paths(paths) => Ok(WatchSignal::Paths(paths)),
            WatcherPoll::Idle => Ok(WatchSignal::Idle),
            WatcherPoll::Error(error) => {
                eprintln!("criv watch: watcher error: {error}");
                self.recovery = None;
                self.suspend("watcher adapter error");
                Ok(WatchSignal::Idle)
            }
            WatcherPoll::Disconnected => Err(CrivError::new("watcher event channel disconnected")),
        }
    }

    fn docs_changed(&self, signal: &WatchSignal) -> bool {
        matches!(signal, WatchSignal::Paths(paths) if paths.iter().any(|path| path.starts_with(&self.active.docs_path)))
    }

    fn must_reconfigure(&self, signal: &WatchSignal) -> bool {
        if self.suspended {
            return !matches!(signal, WatchSignal::Idle) || Instant::now() >= self.next_retry;
        }
        let config_changed = match read_config_source(&self.root) {
            Ok(source) => source != self.active.config_source,
            Err(_) => true,
        };
        matches!(signal, WatchSignal::Paths(_))
            && (config_changed
                || WatchTopology::observe(&self.root, &self.active.config) != self.active.topology
                || WatchSet::active(&self.root, &self.active.config) != self.active.watcher.set)
    }

    fn reconfigure(&mut self) {
        let root = self.root.clone();
        let mut recovery_config = None;
        let candidate = (|| {
            let config_source = read_config_source(&root)?;
            let config = Config::parse(config_source.as_deref())?;
            recovery_config = Some(config.clone());
            ActiveWatchGeneration::candidate(
                &root,
                config,
                config_source,
                self.watcher_factory.as_ref(),
            )
        })();
        match candidate {
            Ok(candidate) => {
                if self.suspended {
                    eprintln!("criv watch: reconfiguration recovered");
                }
                self.active = candidate;
                self.recovery = None;
                self.suspended = false;
                self.failure = None;
                self.next_retry = Instant::now();
            }
            Err(failure) => {
                let mut cause = failure.error.to_string();
                if failure.watcher_unavailable {
                    self.recovery = None;
                } else {
                    let recovery_set = WatchSet::recovery(&root, recovery_config.as_ref());
                    match WatchBinding::start(self.watcher_factory.as_ref(), recovery_set) {
                        Ok(recovery) => self.recovery = Some(recovery),
                        Err(watcher_error) => {
                            self.recovery = None;
                            cause.push_str(&format!("; watcher adapter error: {watcher_error}"));
                        }
                    }
                }
                self.suspend(&cause);
            }
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
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn active_watch_set_covers_config_docs_and_source_topology() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("generated")).unwrap();
        let config = Config::parse(Some(
            "[vault]\ndocs = \"docs\"\n\n[source]\nroots = [\"src\", \"generated/api/file.rs\"]\n",
        ))
        .unwrap();

        let set = WatchSet::active(root, &config);

        assert_eq!(
            set.targets,
            vec![
                WatchTarget::non_recursive(root.to_path_buf()),
                WatchTarget::recursive(root.join("docs")),
                WatchTarget::non_recursive(root.join("generated")),
                WatchTarget::recursive(root.join("src")),
            ]
        );
    }

    #[test]
    fn recovery_watch_set_keeps_only_candidate_recovery_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("generated")).unwrap();
        let config = Config::parse(Some(
            "[vault]\ndocs = \"docs\"\n\n[source]\nroots = [\"src\", \"generated/api/file.rs\"]\n",
        ))
        .unwrap();

        let set = WatchSet::recovery(root, Some(&config));

        assert_eq!(
            set.targets,
            vec![
                WatchTarget::non_recursive(root.to_path_buf()),
                WatchTarget::recursive(root.join("docs")),
                WatchTarget::non_recursive(root.join("generated")),
            ]
        );
    }

    #[derive(Debug)]
    struct DisconnectedWatcherFactory;

    impl WatcherFactory for DisconnectedWatcherFactory {
        fn start(&self, _set: &WatchSet) -> Result<Box<dyn WatcherAdapter>> {
            Ok(Box::new(DisconnectedWatcher))
        }
    }

    #[derive(Debug)]
    struct DisconnectedWatcher;

    impl WatcherAdapter for DisconnectedWatcher {
        fn poll(&mut self, _timeout: Duration) -> WatcherPoll {
            WatcherPoll::Disconnected
        }
    }

    #[test]
    fn live_session_treats_its_disconnected_watcher_as_fatal() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(root.join("criv.toml"), "[index]\nsource = false\n").unwrap();
        let mut session =
            LiveWatchSession::start_with_factory(root, Arc::new(DisconnectedWatcherFactory))
                .unwrap();

        let error = session.step(Duration::ZERO).unwrap_err();

        assert_eq!(error.to_string(), "watcher event channel disconnected");
    }

    #[derive(Debug)]
    struct ScriptedWatcherFactory {
        starts: Mutex<Vec<WatchSet>>,
        scripts: Mutex<VecDeque<VecDeque<WatcherPoll>>>,
    }

    impl ScriptedWatcherFactory {
        fn new(scripts: Vec<Vec<WatcherPoll>>) -> Self {
            Self {
                starts: Mutex::new(Vec::new()),
                scripts: Mutex::new(
                    scripts
                        .into_iter()
                        .map(VecDeque::from)
                        .collect::<VecDeque<_>>(),
                ),
            }
        }
    }

    impl WatcherFactory for ScriptedWatcherFactory {
        fn start(&self, set: &WatchSet) -> Result<Box<dyn WatcherAdapter>> {
            self.starts.lock().unwrap().push(set.clone());
            let polls = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            Ok(Box::new(ScriptedWatcher { polls }))
        }
    }

    #[derive(Debug)]
    struct ScriptedWatcher {
        polls: VecDeque<WatcherPoll>,
    }

    impl WatcherAdapter for ScriptedWatcher {
        fn poll(&mut self, _timeout: Duration) -> WatcherPoll {
            self.polls.pop_front().unwrap_or(WatcherPoll::Idle)
        }
    }

    #[test]
    fn live_session_restarts_the_watcher_after_an_adapter_error() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(root.join("criv.toml"), "[index]\nsource = false\n").unwrap();
        let factory = Arc::new(ScriptedWatcherFactory::new(vec![
            vec![WatcherPoll::Error("injected watcher failure".into())],
            vec![],
        ]));
        let mut session = LiveWatchSession::start_with_factory(root, factory.clone()).unwrap();

        session.step(Duration::ZERO).unwrap();
        assert_eq!(factory.starts.lock().unwrap().len(), 1);

        session.next_retry = Instant::now();
        session.step(Duration::ZERO).unwrap();

        assert_eq!(factory.starts.lock().unwrap().len(), 2);
        assert!(!session.suspended);
    }

    #[derive(Debug, Default)]
    struct FailingWatcherFactory {
        starts: AtomicUsize,
    }

    impl WatcherFactory for FailingWatcherFactory {
        fn start(&self, _set: &WatchSet) -> Result<Box<dyn WatcherAdapter>> {
            let attempt = self.starts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return Ok(Box::new(ScriptedWatcher {
                    polls: VecDeque::from([WatcherPoll::Error("adapter stopped".into())]),
                }));
            }
            Err(CrivError::new("injected adapter start failure"))
        }
    }

    #[test]
    fn watcher_creation_failure_is_retried_only_once_per_interval() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(root.join("criv.toml"), "[index]\nsource = false\n").unwrap();
        let factory = Arc::new(FailingWatcherFactory::default());
        let mut session = LiveWatchSession::start_with_factory(root, factory.clone()).unwrap();
        session.step(Duration::ZERO).unwrap();

        session.next_retry = Instant::now();
        session.step(Duration::ZERO).unwrap();

        assert_eq!(factory.starts.load(Ordering::SeqCst), 2);
    }

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
        let (_, mut file) =
            open_regular_file_in(root, Path::new(".criv"), Path::new(".criv/watch.lock")).unwrap();
        file.write_all(b"not a lock record\n").unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut lock = super::WatchSessionLock::acquire(root, super::WatchMode::Live).unwrap();

        lock._file.rewind().unwrap();
        let mut contents = String::new();
        lock._file.read_to_string(&mut contents).unwrap();
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

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use notify_debouncer_mini::{
    DebounceEventResult, Debouncer, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};
use serde::Serialize;
use usage::{Args as UsageArgs, ValueEnum};

use crate::config::Config;
use crate::discovery::SourceEventFilter;
use crate::refresh::{RefreshCause, RefreshSession};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
enum Format {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Default, UsageArgs)]
pub struct WatchOptions {
    #[usage(long)]
    once: bool,
    /// Select the human summary or a JSON refresh report. `--once` only.
    #[usage(long, value_enum, default = "text")]
    format: Format,
}

pub fn run(root: &Path, options: &WatchOptions) -> Result<()> {
    let files = RepositoryFiles::open_vault(root)?;
    let mode = if options.once {
        WatchMode::Once
    } else {
        WatchMode::Live
    };
    let _lock = WatchSessionLock::acquire_from(&files, mode)?;
    if options.once {
        run_once(&files, options.format)?;
        return Ok(());
    }
    let mut session = LiveWatchSession::start_from(&files)?;

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
    path_mappings: Vec<(PathBuf, PathBuf)>,
}

impl NotifyWatcherAdapter {
    fn start(set: &WatchSet) -> Result<Self> {
        let (tx, receiver) = mpsc::channel::<DebounceEventResult>();
        let mut debouncer = new_debouncer(Duration::from_millis(250), move |event| {
            let _ = tx.send(event);
        })
        .map_err(|err| CrivError::new(format!("failed to start watcher: {err}")))?;
        let mut path_mappings = Vec::new();
        for target in &set.targets {
            let watch_path = target.path.canonicalize().map_err(|err| {
                CrivError::new(format!(
                    "failed to resolve watch target {}: {err}",
                    target.path.display()
                ))
            })?;
            let mode = match target.depth {
                WatchDepth::NonRecursive => RecursiveMode::NonRecursive,
                WatchDepth::Recursive => RecursiveMode::Recursive,
            };
            debouncer
                .watcher()
                .watch(&watch_path, mode)
                .map_err(|err| {
                    CrivError::new(format!("failed to watch {}: {err}", target.path.display()))
                })?;
            path_mappings.push((watch_path, target.path.clone()));
        }
        path_mappings.sort_by(|left, right| {
            right
                .0
                .components()
                .count()
                .cmp(&left.0.components().count())
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(Self {
            _debouncer: debouncer,
            receiver,
            path_mappings,
        })
    }
}

impl WatcherAdapter for NotifyWatcherAdapter {
    fn poll(&mut self, timeout: Duration) -> WatcherPoll {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(events)) if events.is_empty() => WatcherPoll::Idle,
            Ok(Ok(events)) => WatcherPoll::Paths(
                events
                    .into_iter()
                    .map(|event| logical_event_path(&event.path, &self.path_mappings))
                    .collect(),
            ),
            Ok(Err(err)) => WatcherPoll::Error(err.to_string()),
            Err(mpsc::RecvTimeoutError::Timeout) => WatcherPoll::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => WatcherPoll::Disconnected,
        }
    }
}

fn logical_event_path(path: &Path, mappings: &[(PathBuf, PathBuf)]) -> PathBuf {
    mappings
        .iter()
        .find_map(|(watched, logical)| {
            path.strip_prefix(watched)
                .ok()
                .map(|suffix| logical.join(suffix))
        })
        .unwrap_or_else(|| path.to_path_buf())
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
    const fn non_recursive(path: PathBuf) -> Self {
        Self {
            path,
            depth: WatchDepth::NonRecursive,
        }
    }

    const fn recursive(path: PathBuf) -> Self {
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
                if matches!(path_kind(root, &path), PathKind::Missing | PathKind::Unsafe) {
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
    match path_kind(root, requested) {
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
        if candidate.starts_with(root) && path_kind(root, &candidate) == PathKind::Directory {
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
    Rebuild { cause: RefreshCause },
    Continue,
}

const fn watch_decision(docs_changed: bool, source_changed: bool) -> WatchDecision {
    match (docs_changed, source_changed) {
        (_, true) => WatchDecision::Rebuild {
            cause: RefreshCause::SourceChanged,
        },
        (true, false) => WatchDecision::Rebuild {
            cause: RefreshCause::DocsChanged,
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
    source_filter: SourceEventFilter,
}

#[derive(Debug)]
struct CandidateFailure {
    error: CrivError,
    watcher_unavailable: bool,
}

impl CandidateFailure {
    const fn candidate(error: CrivError) -> Self {
        Self {
            error,
            watcher_unavailable: false,
        }
    }

    const fn watcher(error: CrivError) -> Self {
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
        files: &RepositoryFiles,
        root: &Path,
        config: Config,
        config_source: Option<String>,
        factory: &dyn WatcherFactory,
    ) -> std::result::Result<Self, CandidateFailure> {
        let docs_path = config.docs_path(root);
        require_real_docs_root(root, &docs_path).map_err(CandidateFailure::candidate)?;
        let topology = WatchTopology::observe(root, &config);
        let watcher = WatchBinding::start(factory, WatchSet::active(root, &config))
            .map_err(CandidateFailure::watcher)?;
        let mut refresh =
            RefreshSession::live_from(files, &config).map_err(CandidateFailure::candidate)?;
        let summary = refresh
            .refresh(RefreshCause::Initial)
            .map_err(CandidateFailure::candidate)?
            .text_summary();
        println!("{summary}");
        let source_filter = SourceEventFilter::new(root, &config);
        Ok(Self {
            config,
            config_source,
            refresh,
            docs_path,
            topology,
            watcher,
            source_filter,
        })
    }
}

#[derive(Debug)]
struct LiveWatchSession {
    files: RepositoryFiles,
    root: PathBuf,
    active: ActiveWatchGeneration,
    recovery: Option<WatchBinding>,
    watcher_factory: Arc<dyn WatcherFactory>,
    suspended: bool,
    failure: Option<String>,
    next_retry: Instant,
}

impl LiveWatchSession {
    fn start_from(files: &RepositoryFiles) -> Result<Self> {
        Self::start_with_factory(files, Arc::new(NotifyWatcherFactory))
    }

    fn start_with_factory(
        files: &RepositoryFiles,
        watcher_factory: Arc<dyn WatcherFactory>,
    ) -> Result<Self> {
        let root = files.root();
        let config_source = read_config_source(files)?;
        let config = Config::parse(config_source.as_deref())?;
        let active = ActiveWatchGeneration::candidate(
            files,
            root,
            config,
            config_source,
            watcher_factory.as_ref(),
        )
        .map_err(|failure| failure.error)?;
        Ok(Self {
            files: files.clone(),
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

        let docs_changed = self.docs_changed(&signal);
        let source_changed = self.source_changed(&signal);
        if let WatchDecision::Rebuild { cause } = watch_decision(docs_changed, source_changed) {
            let expected_config_source = self.active.config_source.clone();
            let result =
                self.active.refresh.refresh_with_precommit_check(
                    cause,
                    || match read_config_source(&self.files) {
                        Ok(source) if source == expected_config_source => Ok(()),
                        Ok(_) => Err(CrivError::new(
                            "watch configuration changed before State publication",
                        )),
                        Err(err) => Err(err),
                    },
                );
            if read_config_source(&self.files).ok() != Some(self.active.config_source.clone()) {
                self.reconfigure();
                return Ok(());
            }
            match result {
                Ok(refreshed) => println!("{}", refreshed.text_summary()),
                Err(err) => eprintln!("criv watch: {err}"),
            }
        }
        Ok(())
    }

    fn poll(&mut self, timeout: Duration) -> Result<WatchSignal> {
        let poll = if self.suspended {
            if let Some(recovery) = self.recovery.as_mut() {
                recovery.adapter.poll(timeout)
            } else {
                if !timeout.is_zero() {
                    std::thread::sleep(timeout);
                }
                WatcherPoll::Idle
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

    fn source_changed(&self, signal: &WatchSignal) -> bool {
        matches!(signal, WatchSignal::Paths(paths) if paths.iter().any(|path| {
            self.active.source_filter.relevant(path)
        }))
    }

    fn must_reconfigure(&self, signal: &WatchSignal) -> bool {
        if self.suspended {
            return !matches!(signal, WatchSignal::Idle) || Instant::now() >= self.next_retry;
        }
        if !matches!(signal, WatchSignal::Paths(_)) {
            return false;
        }
        let config_changed = read_config_source(&self.files)
            .map_or(true, |source| source != self.active.config_source);
        config_changed
            || WatchTopology::observe(&self.root, &self.active.config) != self.active.topology
            || WatchSet::active(&self.root, &self.active.config) != self.active.watcher.set
    }

    fn reconfigure(&mut self) {
        let root = self.root.clone();
        let mut recovery_config = None;
        let candidate = (|| {
            let config_source = read_config_source(&self.files)?;
            let config = Config::parse(config_source.as_deref())?;
            recovery_config = Some(config.clone());
            ActiveWatchGeneration::candidate(
                &self.files,
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
                            cause = format!("{cause}; watcher adapter error: {watcher_error}");
                        }
                    }
                }
                self.suspend(&cause);
            }
        }
    }

    fn suspend(&mut self, cause: &str) {
        self.suspended = true;
        self.next_retry = Instant::now()
            .checked_add(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        if self.failure.as_deref() != Some(cause) {
            eprintln!("criv watch: reconfiguration failed: {cause}; keeping last successful State");
            self.failure = Some(cause.to_string());
        }
    }
}

fn read_config_source(files: &RepositoryFiles) -> Result<Option<String>> {
    files.read_optional_string(Path::new("criv.toml"))
}

impl WatchTopology {
    fn observe(root: &Path, config: &Config) -> Self {
        let mut paths = config
            .source_roots
            .iter()
            .map(|path| (path.clone(), path_kind(root, &root.join(path))))
            .collect::<Vec<_>>();
        paths.push((
            config.docs_dir.clone(),
            path_kind(root, &config.docs_path(root)),
        ));
        paths.sort();
        Self { paths }
    }
}

fn path_kind(root: &Path, path: &Path) -> PathKind {
    let Ok(relative) = path.strip_prefix(root) else {
        return PathKind::Unsafe;
    };
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return exact_path_kind(root);
    }
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let kind = exact_path_kind(&current);
        if index.saturating_add(1) == components.len() {
            return kind;
        }
        match kind {
            PathKind::Directory => {}
            PathKind::Missing => return PathKind::Missing,
            PathKind::File | PathKind::Unsafe => return PathKind::Unsafe,
        }
    }
    PathKind::Unsafe
}

fn exact_path_kind(path: &Path) -> PathKind {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || is_junction(path) => PathKind::Unsafe,
        Ok(metadata) if metadata.is_file() => PathKind::File,
        Ok(metadata) if metadata.is_dir() => PathKind::Directory,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PathKind::Missing,
        Err(_) | Ok(_) => PathKind::Unsafe,
    }
}

fn require_real_docs_root(root: &Path, path: &Path) -> Result<()> {
    match path_kind(root, path) {
        PathKind::Directory => Ok(()),
        PathKind::File | PathKind::Unsafe => Err(CrivError::new(format!(
            "configured docs root {} must be a real directory",
            path.display()
        ))),
        PathKind::Missing => Err(CrivError::new(format!(
            "configured docs root {} does not exist",
            path.display()
        ))),
    }
}

#[cfg(windows)]
fn is_junction(path: &Path) -> bool {
    junction::exists(path).unwrap_or(false)
}

#[cfg(not(windows))]
const fn is_junction(_path: &Path) -> bool {
    false
}

/// A single `criv watch --once` rebuild, warmed by the on-disk source graph
/// cache left behind by the previous run.
fn run_once(files: &RepositoryFiles, format: Format) -> Result<()> {
    let mut refresh = RefreshSession::one_shot_from(files)?;
    let result = refresh.refresh(RefreshCause::Initial)?;
    match format {
        Format::Text => {
            println!("{}", result.text_summary());
            println!("next: criv check");
        }
        Format::Json => {
            let report = RefreshReport {
                ok: result.errors() == 0,
                snapshot: result.snapshot(),
                errors: result.errors(),
                warnings: result.warnings(),
                next: "criv check",
            };
            let json = serde_json::to_string_pretty(&report).map_err(|err| {
                CrivError::new(format!("failed to serialize refresh report: {err}"))
            })?;
            println!("{json}");
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct RefreshReport<'a> {
    ok: bool,
    snapshot: &'a str,
    errors: usize,
    warnings: usize,
    next: &'a str,
}

#[derive(Debug, Clone, Copy)]
enum WatchMode {
    Live,
    Once,
}

impl WatchMode {
    const fn label(self) -> &'static str {
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
    #[cfg(test)]
    fn acquire(root: &Path, mode: WatchMode) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::acquire_from(&files, mode)
    }

    fn acquire_from(files: &RepositoryFiles, mode: WatchMode) -> Result<Self> {
        let requested_path = files.root().join(".criv/watch.lock");
        let (_, mut file) = files
            .write_scope(Path::new(".criv"))?
            .open_regular_file(Path::new(".criv/watch.lock"))
            .map_err(|err| {
                CrivError::new(format!(
                    "unsafe watch lock path {}: {err}",
                    requested_path.display()
                ))
            })?;

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
            .and_then(|()| file.rewind())
            .and_then(|()| file.write_all(record.as_bytes()))
            .and_then(|()| file.sync_all())
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
    let [schema, pid, mode] = lines.as_slice() else {
        return None;
    };
    if *schema != "schema criv.watch-lock.v1" {
        return None;
    }
    let pid = pid.strip_prefix("pid ")?.parse().ok()?;
    let mode = match mode.strip_prefix("mode ")? {
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

    fn watcher_reports_path(mut watcher: NotifyWatcherAdapter, expected: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let WatcherPoll::Paths(paths) = watcher.poll(Duration::from_millis(250))
                && paths.iter().any(|path| path == expected)
            {
                return true;
            }
        }
        false
    }

    #[test]
    fn notify_watcher_reports_a_recursive_child_change() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("src");
        fs::create_dir(&source).unwrap();
        let set = WatchSet {
            targets: vec![WatchTarget::recursive(source.clone())],
        };
        let watcher = NotifyWatcherAdapter::start(&set).unwrap();
        let changed = source.join("changed.rs");

        fs::write(&changed, "pub fn changed() {}\n").unwrap();

        assert!(watcher_reports_path(watcher, &changed));
    }

    #[test]
    fn notify_watcher_reports_a_change_with_overlapping_targets() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("src");
        fs::create_dir(&source).unwrap();
        let set = WatchSet {
            targets: vec![
                WatchTarget::non_recursive(temp.path().to_path_buf()),
                WatchTarget::recursive(source.clone()),
            ],
        };
        let watcher = NotifyWatcherAdapter::start(&set).unwrap();
        let changed = source.join("changed.rs");

        fs::write(&changed, "pub fn changed() {}\n").unwrap();

        assert!(watcher_reports_path(watcher, &changed));
    }

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
        let files = RepositoryFiles::open(root).unwrap();
        let mut session =
            LiveWatchSession::start_with_factory(&files, Arc::new(DisconnectedWatcherFactory))
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
        let files = RepositoryFiles::open(root).unwrap();
        let mut session = LiveWatchSession::start_with_factory(&files, factory.clone()).unwrap();

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
        let files = RepositoryFiles::open(root).unwrap();
        let mut session = LiveWatchSession::start_with_factory(&files, factory.clone()).unwrap();
        session.step(Duration::ZERO).unwrap();

        session.next_retry = Instant::now();
        session.step(Duration::ZERO).unwrap();

        assert_eq!(factory.starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn watch_decisions_distinguish_docs_and_source_changes() {
        assert_eq!(
            watch_decision(true, false),
            WatchDecision::Rebuild {
                cause: RefreshCause::DocsChanged
            }
        );
        assert_eq!(
            watch_decision(false, true),
            WatchDecision::Rebuild {
                cause: RefreshCause::SourceChanged
            }
        );
        assert_eq!(
            watch_decision(true, true),
            WatchDecision::Rebuild {
                cause: RefreshCause::SourceChanged
            }
        );
        assert_eq!(watch_decision(false, false), WatchDecision::Continue);
    }

    #[cfg(unix)]
    #[test]
    fn watch_topology_rejects_a_linked_ancestor() {
        use std::os::unix::fs::symlink;

        let repository = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::create_dir(outside.path().join("docs")).unwrap();
        symlink(outside.path(), repository.path().join("linked")).unwrap();

        assert_eq!(
            path_kind(repository.path(), &repository.path().join("linked/docs")),
            PathKind::Unsafe
        );
        assert!(
            require_real_docs_root(repository.path(), &repository.path().join("linked/docs"))
                .is_err()
        );
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
        let files = RepositoryFiles::open(root).unwrap();
        let (_, mut file) = files
            .write_scope(Path::new(".criv"))
            .unwrap()
            .open_regular_file(Path::new(".criv/watch.lock"))
            .unwrap();
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

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use criv_state_wire::layout;
use notify_debouncer_mini::{
    DebounceEventResult, Debouncer, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};
use serde::Serialize;
use usage_rs::{Args as UsageArgs, ValueEnum};

use crate::config::Config;
use crate::discovery::{SourceEventFilter, VaultEventFilter};
use crate::repository::{self, EntryKind, RepositoryFiles};
use crate::{CrivError, Result};

mod refresh;

use refresh::{RefreshCause, RefreshSession};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEBOUNCE: Duration = Duration::from_millis(250);
const RECONFIGURATION_RETRY: Duration = Duration::from_secs(1);

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

/// Acquire the Watch lock before starting a single refresh or a live session.
pub fn run(root: &Path, options: &WatchOptions) -> Result<()> {
    let files = crate::config::open_vault(root)?;
    let mode = if options.once {
        WatchMode::Once
    } else {
        WatchMode::Live
    };
    let _lock = WatchSessionLock::acquire(&files, mode)?;
    if options.once {
        run_once(&files, options.format)?;
        return Ok(());
    }
    let mut session = LiveWatchSession::start(&files, Arc::new(NotifyWatcherFactory))?;

    println!("criv watch running");

    loop {
        session.step(POLL_INTERVAL)?;
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
        let mut debouncer = new_debouncer(DEBOUNCE, move |event| {
            // The receiver drops only with this adapter, so no one awaits a lost event.
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WatchedRole {
    Docs,
    Source,
}

/// The configured locations a watch generation observes: the docs root and
/// each Source root, relative to the repository root.
fn watched_locations(config: &Config) -> impl Iterator<Item = (WatchedRole, &str)> {
    std::iter::once((WatchedRole::Docs, config.docs_dir())).chain(
        config
            .source_roots()
            .iter()
            .map(|source_root| (WatchedRole::Source, source_root.as_str())),
    )
}

impl WatchSet {
    fn active(files: &RepositoryFiles, config: &Config) -> Self {
        let root = files.root();
        let mut targets = BTreeMap::new();
        insert_watch_target(&mut targets, root.to_path_buf(), WatchDepth::NonRecursive);
        for (_, location) in watched_locations(config) {
            insert_existing_or_ancestor(&mut targets, files, &root.join(location));
        }
        Self::from_targets(targets)
    }

    /// While a generation is suspended, watch the docs root as an active
    /// generation does, and only the nearest existing directory of each
    /// missing Source root.
    fn recovery(files: &RepositoryFiles, config: Option<&Config>) -> Self {
        let root = files.root();
        let mut targets = BTreeMap::new();
        insert_watch_target(&mut targets, root.to_path_buf(), WatchDepth::NonRecursive);
        for (role, location) in config.into_iter().flat_map(watched_locations) {
            let path = root.join(location);
            match role {
                WatchedRole::Docs => insert_existing_or_ancestor(&mut targets, files, &path),
                WatchedRole::Source
                    if matches!(
                        path_kind(files, &path),
                        EntryKind::Missing | EntryKind::Other
                    ) =>
                {
                    insert_watch_target(
                        &mut targets,
                        nearest_existing_directory(files, &path),
                        WatchDepth::NonRecursive,
                    );
                }
                WatchedRole::Source => {}
            }
        }
        Self::from_targets(targets)
    }

    fn from_targets(targets: BTreeMap<PathBuf, WatchDepth>) -> Self {
        Self {
            targets: targets
                .into_iter()
                .map(|(path, depth)| WatchTarget { path, depth })
                .collect(),
        }
    }
}

fn insert_existing_or_ancestor(
    targets: &mut BTreeMap<PathBuf, WatchDepth>,
    files: &RepositoryFiles,
    requested: &Path,
) {
    match path_kind(files, requested) {
        EntryKind::Directory => {
            insert_watch_target(targets, requested.to_path_buf(), WatchDepth::Recursive);
        }
        EntryKind::File => {
            insert_watch_target(targets, requested.to_path_buf(), WatchDepth::NonRecursive);
        }
        EntryKind::Missing | EntryKind::Other => {
            insert_watch_target(
                targets,
                nearest_existing_directory(files, requested),
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

fn nearest_existing_directory(files: &RepositoryFiles, requested: &Path) -> PathBuf {
    let root = files.root();
    let mut candidate = requested.to_path_buf();
    loop {
        if candidate.starts_with(root) && path_kind(files, &candidate) == EntryKind::Directory {
            return candidate;
        }
        if candidate == root || !candidate.pop() {
            return root.to_path_buf();
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WatchTopology {
    paths: Vec<(String, EntryKind)>,
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
    topology: WatchTopology,
    watcher: WatchBinding,
    source_filter: SourceEventFilter,
    vault_filter: VaultEventFilter,
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

#[derive(Debug)]
enum GenerationRefresh {
    Summary(String),
    Failed(CrivError),
    Reconfigure,
}

impl ActiveWatchGeneration {
    /// Guard publication with this generation's config and prefer reconfiguration
    /// if a second read detects a change, even when the refresh failed.
    /// A configuration that changed, or that cannot be read, is a reconfiguration:
    /// the session reads it again and reports that read's failure.
    fn refresh(&mut self, files: &RepositoryFiles, cause: RefreshCause) -> GenerationRefresh {
        let expected = &self.config_source;
        let mut changed_before_commit = false;
        let result = self.refresh.refresh(cause, || {
            let observed = read_config_source(files)?;
            if observed == *expected {
                return Ok(());
            }
            changed_before_commit = true;
            Err(CrivError::new(
                "watch configuration changed before State publication",
            ))
        });
        let unchanged = !changed_before_commit
            && read_config_source(files).is_ok_and(|source| source == self.config_source);
        if !unchanged {
            return GenerationRefresh::Reconfigure;
        }
        match result {
            Ok(refreshed) => GenerationRefresh::Summary(refreshed.text_summary()),
            Err(err) => GenerationRefresh::Failed(err),
        }
    }

    /// Start watching before the initial refresh so changes during its build stay queued.
    fn candidate(
        files: &RepositoryFiles,
        config: Config,
        config_source: Option<String>,
        factory: &dyn WatcherFactory,
    ) -> std::result::Result<Self, CandidateFailure> {
        let root = files.root();
        require_real_docs_root(files, &config.docs_path(root))
            .map_err(CandidateFailure::candidate)?;
        let topology = WatchTopology::observe(files, &config);
        let watcher = WatchBinding::start(factory, WatchSet::active(files, &config))
            .map_err(CandidateFailure::watcher)?;
        let mut refresh = RefreshSession::new(files, &config);
        let summary = refresh
            .refresh(RefreshCause::Initial, || Ok(()))
            .map_err(CandidateFailure::candidate)?
            .text_summary();
        println!("{summary}");
        let source_filter =
            SourceEventFilter::new(root, &config).map_err(CandidateFailure::candidate)?;
        let vault_filter = VaultEventFilter::new(root, &config);
        Ok(Self {
            config,
            config_source,
            refresh,
            topology,
            watcher,
            source_filter,
            vault_filter,
        })
    }
}

#[derive(Debug)]
struct LiveWatchSession {
    files: RepositoryFiles,
    active: ActiveWatchGeneration,
    recovery: Option<WatchBinding>,
    watcher_factory: Arc<dyn WatcherFactory>,
    suspended: bool,
    failure: Option<String>,
    next_retry: Instant,
}

impl LiveWatchSession {
    fn start(files: &RepositoryFiles, watcher_factory: Arc<dyn WatcherFactory>) -> Result<Self> {
        let config_source = read_config_source(files)?;
        let config = Config::parse(config_source.as_deref())?;
        let active = ActiveWatchGeneration::candidate(
            files,
            config,
            config_source,
            watcher_factory.as_ref(),
        )
        .map_err(|failure| failure.error)?;
        Ok(Self {
            files: files.clone(),
            active,
            recovery: None,
            watcher_factory,
            suspended: false,
            failure: None,
            next_retry: Instant::now(),
        })
    }

    /// Process one event batch, replacing stale configuration before ordinary refreshes.
    fn step(&mut self, timeout: Duration) -> Result<()> {
        let paths = self.poll(timeout)?;
        if self.must_reconfigure(&paths) {
            self.reconfigure();
            return Ok(());
        }
        if self.suspended {
            return Ok(());
        }

        let cause = if paths
            .iter()
            .any(|path| self.active.source_filter.relevant(path))
        {
            RefreshCause::SourceChanged
        } else if paths
            .iter()
            .any(|path| self.active.vault_filter.relevant(path))
        {
            RefreshCause::DocsChanged
        } else {
            return Ok(());
        };
        match self.active.refresh(&self.files, cause) {
            GenerationRefresh::Summary(summary) => println!("{summary}"),
            GenerationRefresh::Failed(err) => eprintln!("criv watch: {err}"),
            GenerationRefresh::Reconfigure => self.reconfigure(),
        }
        Ok(())
    }

    /// The changed paths of one event batch; an idle poll returns none.
    fn poll(&mut self, timeout: Duration) -> Result<Vec<PathBuf>> {
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
            WatcherPoll::Paths(paths) => Ok(paths),
            WatcherPoll::Idle => Ok(Vec::new()),
            WatcherPoll::Error(error) => {
                eprintln!("criv watch: watcher error: {error}");
                self.recovery = None;
                self.suspend("watcher adapter error");
                Ok(Vec::new())
            }
            WatcherPoll::Disconnected => Err(CrivError::new("watcher event channel disconnected")),
        }
    }

    fn must_reconfigure(&self, paths: &[PathBuf]) -> bool {
        if self.suspended {
            return !paths.is_empty() || Instant::now() >= self.next_retry;
        }
        if paths.is_empty() {
            return false;
        }
        let config_changed = read_config_source(&self.files)
            .map_or(true, |source| source != self.active.config_source);
        config_changed
            || WatchTopology::observe(&self.files, &self.active.config) != self.active.topology
            || WatchSet::active(&self.files, &self.active.config) != self.active.watcher.set
    }

    fn reconfigure(&mut self) {
        let mut recovery_config = None;
        let candidate = (|| {
            let config_source = read_config_source(&self.files)?;
            let config = Config::parse(config_source.as_deref())?;
            recovery_config = Some(config.clone());
            ActiveWatchGeneration::candidate(
                &self.files,
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
                    let recovery_set = WatchSet::recovery(&self.files, recovery_config.as_ref());
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
            .checked_add(RECONFIGURATION_RETRY)
            .unwrap_or_else(Instant::now);
        if self.failure.as_deref() != Some(cause) {
            eprintln!("criv watch: reconfiguration failed: {cause}; keeping last successful State");
            self.failure = Some(cause.to_string());
        }
    }
}

/// Read the exact configuration text so a generation can detect any replacement.
fn read_config_source(files: &RepositoryFiles) -> Result<Option<String>> {
    files.read_optional_string(Path::new(crate::config::CONFIG_FILE))
}

impl WatchTopology {
    fn observe(files: &RepositoryFiles, config: &Config) -> Self {
        let root = files.root();
        let mut paths = watched_locations(config)
            .map(|(_, location)| (location.to_string(), path_kind(files, &root.join(location))))
            .collect::<Vec<_>>();
        paths.sort();
        Self { paths }
    }
}

/// The kind of an absolute path below the repository root; a path outside it
/// is `Other`.
fn path_kind(files: &RepositoryFiles, path: &Path) -> EntryKind {
    path.strip_prefix(files.root())
        .map_or(EntryKind::Other, |relative| files.entry_kind(relative))
}

fn require_real_docs_root(files: &RepositoryFiles, path: &Path) -> Result<()> {
    match path_kind(files, path) {
        EntryKind::Directory => Ok(()),
        EntryKind::File | EntryKind::Other => Err(CrivError::new(format!(
            "configured docs root {} must be a real directory",
            path.display()
        ))),
        EntryKind::Missing => Err(CrivError::new(format!(
            "configured docs root {} does not exist",
            path.display()
        ))),
    }
}

/// A single `criv watch --once` rebuild, warmed by the on-disk source graph
/// cache left behind by the previous run.
fn run_once(files: &RepositoryFiles, format: Format) -> Result<()> {
    let mut refresh = RefreshSession::new(files, &Config::load_from(files)?);
    let result = refresh.refresh(RefreshCause::Initial, || Ok(()))?;
    match format {
        Format::Text => {
            println!("{}", result.text_summary());
            println!("next: criv check");
        }
        Format::Json => {
            let report = RefreshReport {
                ok: result.errors == 0,
                snapshot: &result.snapshot,
                errors: result.errors,
                warnings: result.warnings,
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WatchMode {
    Live,
    Once,
}

impl WatchMode {
    const ALL: [Self; 2] = [Self::Live, Self::Once];

    const fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Once => "once",
        }
    }
}

/// The diagnostic `.criv/watch.lock` record of ADR-0095. It names the owner
/// for a contention message and never decides ownership.
#[derive(Debug, Eq, PartialEq)]
struct WatchLockRecord {
    pid: u32,
    mode: WatchMode,
}

impl WatchLockRecord {
    const SCHEMA: &'static str = "schema criv.watch-lock.v1";

    fn render(&self) -> String {
        format!(
            "{}\npid {}\nmode {}\n",
            Self::SCHEMA,
            self.pid,
            self.mode.label()
        )
    }

    fn parse(contents: &str) -> Option<Self> {
        let lines = contents.lines().collect::<Vec<_>>();
        let [schema, pid, mode] = lines.as_slice() else {
            return None;
        };
        if *schema != Self::SCHEMA {
            return None;
        }
        let pid = pid.strip_prefix("pid ")?.parse().ok()?;
        let mode = mode.strip_prefix("mode ")?;
        let mode = WatchMode::ALL
            .into_iter()
            .find(|candidate| candidate.label() == mode)?;
        Some(Self { pid, mode })
    }

    fn read(file: &mut fs::File) -> Option<Self> {
        file.rewind().ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;
        Self::parse(&contents)
    }
}

#[derive(Debug)]
struct WatchSessionLock {
    _file: fs::File,
}

impl WatchSessionLock {
    fn acquire(files: &RepositoryFiles, mode: WatchMode) -> Result<Self> {
        let requested_path = files.root().join(layout::WATCH_LOCK);
        let (_, mut file) = files
            .write_scope(Path::new(layout::DIR))?
            .open_regular_file(Path::new(layout::WATCH_LOCK))
            .map_err(|err| {
                CrivError::new(format!(
                    "unsafe watch lock path {}: {err}",
                    requested_path.display()
                ))
            })?;

        match repository::lock_within(&file, Duration::ZERO) {
            Ok(true) => {}
            Ok(false) => {
                let detail = WatchLockRecord::read(&mut file).map_or_else(String::new, |owner| {
                    format!(" (mode {}, pid {})", owner.mode.label(), owner.pid)
                });
                return Err(CrivError::new(format!(
                    "another watch session owns State refresh{detail}; do not start another watch or run `criv watch --once` while it is active"
                )));
            }
            Err(err) => {
                return Err(CrivError::new(format!(
                    "failed to acquire operating-system watch lock at {}: {err}",
                    requested_path.display()
                )));
            }
        }

        let record = WatchLockRecord {
            pid: std::process::id(),
            mode,
        }
        .render();
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use tempfile::TempDir;

    use super::*;

    const fn recursive(path: PathBuf) -> WatchTarget {
        WatchTarget {
            path,
            depth: WatchDepth::Recursive,
        }
    }

    const fn flat(path: PathBuf) -> WatchTarget {
        WatchTarget {
            path,
            depth: WatchDepth::NonRecursive,
        }
    }

    /// Upper bound for the operating system to deliver one file event.
    const NOTIFY_EVENT_BOUND: Duration = Duration::from_secs(3);

    fn watcher_reports_path(mut watcher: NotifyWatcherAdapter, expected: &Path) -> bool {
        let deadline = Instant::now()
            .checked_add(NOTIFY_EVENT_BOUND)
            .unwrap_or_else(Instant::now);
        while Instant::now() < deadline {
            if let WatcherPoll::Paths(paths) = watcher.poll(POLL_INTERVAL)
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
            targets: vec![recursive(source.clone())],
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
            targets: vec![flat(temp.path().to_path_buf()), recursive(source.clone())],
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

        let files = RepositoryFiles::open(root).unwrap();
        let root = files.root();
        let set = WatchSet::active(&files, &config);

        assert_eq!(
            set.targets,
            vec![
                flat(root.to_path_buf()),
                recursive(root.join("docs")),
                flat(root.join("generated")),
                recursive(root.join("src")),
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

        let files = RepositoryFiles::open(root).unwrap();
        let root = files.root();
        let set = WatchSet::recovery(&files, Some(&config));

        assert_eq!(
            set.targets,
            vec![
                flat(root.to_path_buf()),
                recursive(root.join("docs")),
                flat(root.join("generated")),
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
            LiveWatchSession::start(&files, Arc::new(DisconnectedWatcherFactory)).unwrap();

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

    /// Start a real refresh session with scripted events and published output.
    fn refresh_fixture() -> (TempDir, RepositoryFiles, LiveWatchSession) {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn initial() {}\n").unwrap();
        fs::write(
            root.join("docs/note.md"),
            "---\nid: note\nkind: doc\ntitle: Note\n---\n\nInitial note.\n",
        )
        .unwrap();
        fs::write(
            root.join("criv.toml"),
            "[source]\nroots = [\"src\"]\n[index]\nsource = true\n",
        )
        .unwrap();
        let files = RepositoryFiles::open(root).unwrap();
        let session =
            LiveWatchSession::start(&files, Arc::new(ScriptedWatcherFactory::new(vec![vec![]])))
                .unwrap();
        (temp, files, session)
    }

    /// Capture all published bytes to detect partial writes after a failed refresh.
    fn published_artifacts(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut paths = vec![root.join(".criv/state.json"), root.join(".criv/latest")];
        paths.extend(
            fs::read_dir(root.join(".criv/snapshots"))
                .unwrap()
                .map(|entry| entry.unwrap().path()),
        );
        paths
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect()
    }

    fn published_snapshot(root: &Path) -> String {
        fs::read_to_string(root.join(layout::LATEST))
            .unwrap()
            .trim()
            .to_string()
    }

    fn one_shot_refresh(files: &RepositoryFiles) {
        RefreshSession::new(files, &Config::load_from(files).unwrap())
            .refresh(RefreshCause::Initial, || Ok(()))
            .unwrap();
    }

    fn append_to_config(files: &RepositoryFiles, line: &str) -> String {
        let path = files.root().join("criv.toml");
        let config = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{config}{line}\n")).unwrap();
        config
    }

    #[test]
    /// Keep the returned summary valid after the generation advances.
    fn ordinary_refresh_publishes_an_owned_summary() {
        let (_temp, files, mut session) = refresh_fixture();
        let before = published_snapshot(files.root());
        fs::write(
            files.root().join("docs/note.md"),
            "---\nid: note\nkind: doc\ntitle: Changed\n---\n",
        )
        .unwrap();
        let outcome = session.active.refresh(&files, RefreshCause::DocsChanged);
        let GenerationRefresh::Summary(summary) = outcome else {
            panic!("{outcome:?}")
        };
        let after = published_snapshot(files.root());
        assert_ne!(before, after);
        assert!(summary.contains(&after));
    }

    #[test]
    fn changed_configuration_rejects_publication_and_preserves_source_retry() {
        let (_temp, files, mut session) = refresh_fixture();
        let before = published_artifacts(files.root());
        let previous = published_snapshot(files.root());
        fs::write(
            files.root().join("src/lib.rs"),
            "pub fn recovered_source() {}\n",
        )
        .unwrap();
        let config = append_to_config(&files, "# changed before publication");
        assert!(matches!(
            session.active.refresh(&files, RefreshCause::SourceChanged),
            GenerationRefresh::Reconfigure
        ));
        assert_eq!(published_artifacts(files.root()), before);
        assert_eq!(published_snapshot(files.root()), previous);
        fs::write(files.root().join("criv.toml"), config).unwrap();
        assert!(matches!(
            session.active.refresh(&files, RefreshCause::DocsChanged),
            GenerationRefresh::Summary(_)
        ));
        let live = fs::read_to_string(files.root().join(".criv/state.json")).unwrap();
        assert!(live.contains("recovered_source"));
        one_shot_refresh(&files);
        assert_eq!(
            fs::read_to_string(files.root().join(".criv/state.json")).unwrap(),
            live
        );
    }

    #[test]
    /// Preserve published output when configuration cannot be read at commit.
    fn unreadable_configuration_requests_reconfiguration_and_rolls_back() {
        let (_temp, files, mut session) = refresh_fixture();
        let before = published_artifacts(files.root());
        let previous = published_snapshot(files.root());
        fs::remove_file(files.root().join("criv.toml")).unwrap();
        fs::create_dir(files.root().join("criv.toml")).unwrap();
        assert!(matches!(
            session.active.refresh(&files, RefreshCause::DocsChanged),
            GenerationRefresh::Reconfigure
        ));
        assert_eq!(published_artifacts(files.root()), before);
        assert_eq!(published_snapshot(files.root()), previous);
    }

    fn corrupt_a_snapshot(files: &RepositoryFiles) {
        fs::write(
            files
                .root()
                .join(".criv/snapshots")
                .join(format!("{}.json", "a".repeat(64))),
            "{}\n",
        )
        .unwrap();
    }

    #[test]
    fn unchanged_configuration_reports_refresh_failure() {
        let (_temp, files, mut session) = refresh_fixture();
        let previous = published_snapshot(files.root());
        corrupt_a_snapshot(&files);
        let before = published_artifacts(files.root());
        assert!(matches!(
            session.active.refresh(&files, RefreshCause::DocsChanged),
            GenerationRefresh::Failed(_)
        ));
        assert_eq!(published_artifacts(files.root()), before);
        assert_eq!(published_snapshot(files.root()), previous);
    }

    #[test]
    fn changed_configuration_takes_priority_over_a_refresh_failure() {
        let (_temp, files, mut session) = refresh_fixture();
        corrupt_a_snapshot(&files);
        let before = published_artifacts(files.root());
        append_to_config(&files, "# changed during a failing refresh");
        assert!(matches!(
            session.active.refresh(&files, RefreshCause::DocsChanged),
            GenerationRefresh::Reconfigure
        ));
        assert_eq!(published_artifacts(files.root()), before);
    }

    #[test]
    fn session_suspends_on_invalid_configuration_and_recovers() {
        let (_temp, files, initial) = refresh_fixture();
        let factory = Arc::new(ScriptedWatcherFactory::new(vec![
            vec![WatcherPoll::Paths(vec![files.root().join("docs/note.md")])],
            vec![],
            vec![],
        ]));
        let mut session = LiveWatchSession::start(&files, factory).unwrap();
        let before = published_artifacts(files.root());
        let previous = published_snapshot(files.root());
        fs::write(
            files.root().join("docs/note.md"),
            "---\nid: note\nkind: doc\ntitle: Recovered\n---\n",
        )
        .unwrap();
        fs::write(files.root().join("criv.toml"), "[index").unwrap();
        session.step(Duration::ZERO).unwrap();
        assert!(session.suspended);
        assert!(
            session
                .failure
                .as_ref()
                .unwrap()
                .contains("failed to parse criv.toml")
        );
        assert_eq!(published_artifacts(files.root()), before);
        assert_eq!(published_snapshot(files.root()), previous);
        fs::write(
            files.root().join("criv.toml"),
            initial.active.config_source.unwrap(),
        )
        .unwrap();
        session.next_retry = Instant::now();
        session.step(Duration::ZERO).unwrap();
        assert!(!session.suspended);
        assert!(session.failure.is_none());
        assert_ne!(published_snapshot(files.root()), previous);
        let live = fs::read_to_string(files.root().join(".criv/state.json")).unwrap();
        one_shot_refresh(&files);
        assert_eq!(
            fs::read_to_string(files.root().join(".criv/state.json")).unwrap(),
            live
        );
    }

    #[test]
    fn event_paths_choose_docs_reuse_or_source_refresh() {
        let (_temp, files, _initial) = refresh_fixture();
        let factory = Arc::new(ScriptedWatcherFactory::new(vec![vec![
            WatcherPoll::Paths(vec![files.root().join("docs/note.md")]),
            WatcherPoll::Paths(vec![files.root().join("src/lib.rs")]),
        ]]));
        let mut session = LiveWatchSession::start(&files, factory).unwrap();
        fs::write(
            files.root().join("src/lib.rs"),
            "pub fn changed_source() {}\n",
        )
        .unwrap();
        fs::write(
            files.root().join("docs/note.md"),
            "---\nid: note\nkind: doc\ntitle: Docs Event Title\n---\n",
        )
        .unwrap();
        let state = || fs::read_to_string(files.root().join(".criv/state.json")).unwrap();

        session.step(Duration::ZERO).unwrap();
        assert!(state().contains("Docs Event Title"));
        assert!(!state().contains("changed_source"));

        session.step(Duration::ZERO).unwrap();
        assert!(state().contains("changed_source"));
    }

    #[derive(Debug)]
    struct ConfigurationDuringCandidate {
        path: PathBuf,
        starts: AtomicUsize,
    }

    impl WatcherFactory for ConfigurationDuringCandidate {
        fn start(&self, _set: &WatchSet) -> Result<Box<dyn WatcherAdapter>> {
            let first = self.starts.fetch_add(1, Ordering::SeqCst) == 0;
            let polls = if first {
                fs::write(&self.path, "[index]\nsource = false\n").unwrap();
                VecDeque::from([WatcherPoll::Paths(vec![self.path.clone()])])
            } else {
                VecDeque::new()
            };
            Ok(Box::new(ScriptedWatcher { polls }))
        }
    }

    #[test]
    fn candidate_uses_its_snapshot_then_observes_the_queued_configuration_event() {
        let (_temp, files, _initial) = refresh_fixture();
        let original = read_config_source(&files).unwrap();
        let factory = Arc::new(ConfigurationDuringCandidate {
            path: files.root().join("criv.toml"),
            starts: AtomicUsize::new(0),
        });
        let mut session = LiveWatchSession::start(&files, factory.clone()).unwrap();
        assert_eq!(session.active.config_source, original);
        assert!(
            fs::read_to_string(files.root().join(".criv/state.json"))
                .unwrap()
                .contains("src/lib.rs")
        );
        session.step(Duration::ZERO).unwrap();
        assert_eq!(factory.starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            session.active.config_source,
            read_config_source(&files).unwrap()
        );
        let live = fs::read_to_string(files.root().join(".criv/state.json")).unwrap();
        one_shot_refresh(&files);
        assert_eq!(
            fs::read_to_string(files.root().join(".criv/state.json")).unwrap(),
            live
        );
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
        let mut session = LiveWatchSession::start(&files, factory.clone()).unwrap();

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
        let mut session = LiveWatchSession::start(&files, factory.clone()).unwrap();
        session.step(Duration::ZERO).unwrap();

        session.next_retry = Instant::now();
        session.step(Duration::ZERO).unwrap();

        assert_eq!(factory.starts.load(Ordering::SeqCst), 2);
    }

    #[cfg(unix)]
    #[test]
    fn watch_topology_rejects_a_linked_ancestor() {
        use std::os::unix::fs::symlink;

        let repository = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::create_dir(outside.path().join("docs")).unwrap();
        symlink(outside.path(), repository.path().join("linked")).unwrap();
        let files = RepositoryFiles::open(repository.path()).unwrap();
        let linked_docs = files.root().join("linked/docs");

        assert_eq!(path_kind(&files, &linked_docs), EntryKind::Other);
        assert!(require_real_docs_root(&files, &linked_docs).is_err());
    }

    fn lock(root: &Path, mode: WatchMode) -> Result<WatchSessionLock> {
        WatchSessionLock::acquire(&RepositoryFiles::open(root)?, mode)
    }

    #[test]
    fn operating_system_lock_allows_exactly_one_owner_and_safe_handoff() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(".criv")).unwrap();
        let first = lock(root, WatchMode::Live).unwrap();

        let error = lock(root, WatchMode::Once).unwrap_err();

        assert!(
            error.to_string().contains(&format!(
                "another watch session owns State refresh (mode live, pid {})",
                std::process::id()
            )),
            "unexpected error: {error}"
        );
        drop(first);
        assert!(root.join(".criv/watch.lock").is_file());

        lock(root, WatchMode::Once).unwrap();
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

        let mut held = lock(root, WatchMode::Live).unwrap();

        held._file.rewind().unwrap();
        let mut contents = String::new();
        held._file.read_to_string(&mut contents).unwrap();
        assert_eq!(
            contents,
            format!(
                "schema criv.watch-lock.v1\npid {}\nmode live\n",
                std::process::id()
            )
        );
        drop(held);
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
                let lock = lock(&root, WatchMode::Live);
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

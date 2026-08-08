use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

#[cfg(test)]
use std::{
    cell::Cell,
    sync::{Mutex, MutexGuard},
    thread_local,
};

use fff_search::file_picker::FilePicker;
use fff_search::{
    FFFMode, FilePickerOptions, FuzzySearchOptions, PaginationArgs, QueryParser, SharedFilePicker,
    SharedFrecency,
};

use crate::config::Config;
use crate::source_paths::{
    SourceRootKind, canonical_source_path, source_metadata, source_root_kind,
};
use crate::util::{GlobMatcher, is_text_file};
use crate::{CrivError, Result};

const SCAN_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) fff_starts: usize,
    pub(crate) observations: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        fff_starts: 0,
        observations: 0,
    }) };
}

#[cfg(test)]
static LIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
static FFF_START_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_live_test() -> MutexGuard<'static, ()> {
    LIVE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
fn record_work(update: impl FnOnce(&mut WorkCounts)) {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        update(&mut next);
        counts.set(next);
    });
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileHit {
    path: String,
    score: i32,
    frecency: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedSource {
    pub(crate) path: String,
    pub(crate) frecency: u32,
}

trait SourceIndex: std::fmt::Debug + Send + Sync {
    fn resolve_partial_path(&self, entries: &[IndexedSource], path: &str)
    -> Option<(String, bool)>;
    fn observe(&self) -> Result<IndexSnapshot>;
}

#[derive(Debug, Clone)]
struct IndexSnapshot {
    entries: Vec<IndexedSource>,
    fingerprint: String,
}

#[derive(Debug, Clone)]
struct SourceIndexHandle {
    adapter: SourceIndexAdapter,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceCatalog {
    handle: SourceIndexHandle,
    entries: Arc<[IndexedSource]>,
    paths: Arc<[String]>,
}

#[derive(Debug, Clone)]
enum SourceIndexAdapter {
    Fff(Arc<FffSourceIndex>),
    Empty(Arc<EmptySourceIndex>),
}

impl SourceIndexHandle {
    fn enabled(index: Arc<FffSourceIndex>) -> Self {
        Self {
            adapter: SourceIndexAdapter::Fff(index),
        }
    }

    fn disabled() -> Self {
        Self {
            adapter: SourceIndexAdapter::Empty(Arc::new(EmptySourceIndex)),
        }
    }

    fn is_enabled(&self) -> bool {
        matches!(self.adapter, SourceIndexAdapter::Fff(_))
    }

    fn as_index(&self) -> &dyn SourceIndex {
        match &self.adapter {
            SourceIndexAdapter::Fff(index) => index.as_ref(),
            SourceIndexAdapter::Empty(index) => index.as_ref(),
        }
    }

    fn catalog(&self, entries: Vec<IndexedSource>) -> SourceCatalog {
        let paths = entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        SourceCatalog {
            handle: self.clone(),
            entries: entries.into(),
            paths: paths.into(),
        }
    }
}

impl SourceCatalog {
    pub(crate) fn disabled() -> Self {
        Self {
            handle: SourceIndexHandle::disabled(),
            entries: Arc::from([]),
            paths: Arc::from([]),
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.handle.is_enabled()
    }

    pub(crate) fn entries(&self) -> &[IndexedSource] {
        &self.entries
    }

    pub(crate) fn paths(&self) -> &[String] {
        &self.paths
    }

    pub(crate) fn resolve_partial_path(&self, path: &str) -> Option<(String, bool)> {
        self.handle
            .as_index()
            .resolve_partial_path(&self.entries, path)
    }
}

#[derive(Debug)]
pub(crate) struct SourceIndexLifecycle {
    handle: SourceIndexHandle,
    change_tracking: ChangeTracking,
}

#[derive(Debug)]
enum ChangeTracking {
    Command,
    Watch { fingerprint: Option<String> },
}

#[derive(Debug)]
pub(crate) struct SourceObservation {
    change: SourceChange,
    catalog: SourceCatalog,
}

impl SourceObservation {
    pub(crate) fn change(&self) -> SourceChange {
        self.change
    }

    pub(crate) fn into_catalog(self) -> SourceCatalog {
        self.catalog
    }
}

impl SourceIndexLifecycle {
    pub(crate) fn for_command(root: &Path, config: &Config) -> Result<Self> {
        Self::new(root, config, SourceIndexLifetime::OneShot)
    }

    pub(crate) fn for_watch(root: &Path, config: &Config) -> Result<Self> {
        Self::new(root, config, SourceIndexLifetime::Live)
    }

    fn new(root: &Path, config: &Config, lifetime: SourceIndexLifetime) -> Result<Self> {
        let handle = if config.source_index {
            SourceIndexHandle::enabled(Arc::new(FffSourceIndex::new(
                root,
                &config.source_roots,
                &config.source_exclude,
                lifetime,
            )?))
        } else {
            SourceIndexHandle::disabled()
        };
        let change_tracking = match lifetime {
            SourceIndexLifetime::OneShot => ChangeTracking::Command,
            SourceIndexLifetime::Live => ChangeTracking::Watch { fingerprint: None },
        };
        Ok(Self {
            handle,
            change_tracking,
        })
    }

    pub(crate) fn observe(&mut self) -> Result<SourceObservation> {
        let snapshot = self.handle.as_index().observe()?;
        let change = if !self.handle.is_enabled() {
            SourceChange::Disabled
        } else {
            match &mut self.change_tracking {
                ChangeTracking::Command => SourceChange::Unchanged,
                ChangeTracking::Watch { fingerprint } => {
                    let changed = fingerprint
                        .replace(snapshot.fingerprint.clone())
                        .is_some_and(|previous| previous != snapshot.fingerprint);
                    if changed {
                        SourceChange::Changed
                    } else {
                        SourceChange::Unchanged
                    }
                }
            }
        };
        Ok(SourceObservation {
            change,
            catalog: self.handle.catalog(snapshot.entries),
        })
    }

    #[cfg(test)]
    fn handle(&self) -> SourceIndexHandle {
        self.handle.clone()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourceChange {
    Changed,
    Unchanged,
    Disabled,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SourceIndexLifetime {
    OneShot,
    Live,
}

#[derive(Debug)]
struct FffSourceIndex {
    root: PathBuf,
    source_roots: Vec<String>,
    source_excludes: GlobMatcher,
    pickers: Vec<ScopedPicker>,
    explicit_files: Vec<String>,
    observation_cache: Option<OnceLock<IndexSnapshot>>,
}

#[derive(Debug)]
struct ScopedPicker {
    prefix: String,
    picker: SharedFilePicker,
    _frecency: SharedFrecency,
}

impl FffSourceIndex {
    fn new(
        root: &Path,
        source_roots: &[String],
        source_exclude: &[String],
        lifetime: SourceIndexLifetime,
    ) -> Result<Self> {
        #[cfg(test)]
        let _start_test = FFF_START_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        record_work(|counts| counts.fff_starts += 1);
        let watch = lifetime == SourceIndexLifetime::Live;
        let source_roots = normalize_source_roots(source_roots);
        let source_excludes = GlobMatcher::new(source_exclude)?;
        let scan_plan = SourceScanPlan::new(root, &source_roots)?;
        let mut pickers = Vec::new();
        for scan_root in scan_plan.directories {
            let picker = SharedFilePicker::default();
            let frecency = SharedFrecency::default();
            FilePicker::new_with_shared_state(
                picker.clone(),
                frecency.clone(),
                FilePickerOptions {
                    base_path: root.join(&scan_root.path).to_string_lossy().to_string(),
                    mode: FFFMode::Ai,
                    watch,
                    ..Default::default()
                },
            )
            .map_err(|err| CrivError::new(format!("failed to start fff source index: {err}")))?;

            if !picker.wait_for_indexing_complete(SCAN_TIMEOUT) {
                return Err(CrivError::new("timed out indexing source files with fff"));
            }
            if watch && !picker.wait_for_watcher(SCAN_TIMEOUT) {
                return Err(CrivError::new("timed out starting fff source watcher"));
            }

            pickers.push(ScopedPicker {
                prefix: scan_root.path,
                picker,
                _frecency: frecency,
            });
        }

        Ok(Self {
            root: root.to_path_buf(),
            source_roots,
            source_excludes,
            pickers,
            explicit_files: scan_plan.files,
            observation_cache: (lifetime == SourceIndexLifetime::OneShot).then(OnceLock::new),
        })
    }

    fn with_picker<T>(&self, scoped: &ScopedPicker, f: impl FnOnce(&FilePicker) -> T) -> Result<T> {
        let guard = scoped
            .picker
            .read()
            .map_err(|err| CrivError::new(format!("failed to read fff source index: {err}")))?;
        let picker = guard
            .as_ref()
            .ok_or_else(|| CrivError::new("fff source index is not initialized"))?;
        Ok(f(picker))
    }

    fn indexed_path(&self, path: String) -> Option<String> {
        if self.source_path_allowed(&path) && canonical_source_path(&self.root, &path).is_ok() {
            Some(path)
        } else {
            None
        }
    }

    fn source_path_allowed(&self, path: &str) -> bool {
        !self.source_excludes.is_match(path)
            && self
                .source_roots
                .iter()
                .any(|root| root == "." || path == root || path.starts_with(&format!("{root}/")))
    }

    fn observation(&self) -> Result<IndexSnapshot> {
        if let Some(cache) = &self.observation_cache {
            if let Some(cached) = cache.get() {
                return Ok(cached.clone());
            }
            let observation = self.collect_observation_now()?;
            return Ok(cache.get_or_init(|| observation).clone());
        }
        self.collect_observation_now()
    }

    fn collect_observation_now(&self) -> Result<IndexSnapshot> {
        #[cfg(test)]
        record_work(|counts| counts.observations += 1);

        let mut observed = BTreeMap::new();
        for scoped in &self.pickers {
            observed.extend(self.with_picker(scoped, |picker| {
                picker
                    .get_files()
                    .iter()
                    .filter(|file| !file.is_binary() && !file.is_deleted())
                    .filter_map(|file| {
                        let path = prefixed_path(&scoped.prefix, file.relative_path(picker));
                        self.indexed_path(path).map(|path| {
                            let fingerprint = format!("{path}\0{}\0{}", file.size, file.modified);
                            (
                                path.clone(),
                                (file.total_frecency_score().max(0) as u32, fingerprint),
                            )
                        })
                    })
                    .collect::<BTreeMap<_, _>>()
            })?);
        }
        for path in &self.explicit_files {
            if is_text_file(&self.root.join(path)).unwrap_or(false)
                && self.indexed_path(path.clone()).is_some()
            {
                observed.insert(
                    path.clone(),
                    (0, explicit_file_fingerprint(&self.root, path)?),
                );
            }
        }

        let fingerprint = blake3::hash(
            observed
                .values()
                .map(|(_, fingerprint)| fingerprint.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes(),
        )
        .to_hex()
        .to_string();
        let entries = observed
            .into_iter()
            .map(|(path, (frecency, _))| IndexedSource { path, frecency })
            .collect();
        Ok(IndexSnapshot {
            entries,
            fingerprint,
        })
    }

    fn explicit_file_hits(&self, query: &str) -> Vec<FileHit> {
        self.explicit_files
            .iter()
            .filter(|path| self.indexed_path((*path).clone()).is_some())
            .filter_map(|path| {
                fuzzy_score(path, query).map(|score| FileHit {
                    path: path.clone(),
                    score,
                    frecency: 0,
                })
            })
            .collect()
    }

    fn partial_path_candidates(&self, query: &str, limit: usize) -> Result<Vec<FileHit>> {
        let mut hits = self.explicit_file_hits(query);
        for scoped in &self.pickers {
            hits.extend(self.with_picker(scoped, |picker| {
                let parser = QueryParser::default();
                let query = parser.parse(query);
                let results = picker.fuzzy_search(
                    &query,
                    None,
                    FuzzySearchOptions {
                        max_threads: 0,
                        project_path: None,
                        current_file: None,
                        pagination: PaginationArgs { offset: 0, limit },
                        ..Default::default()
                    },
                );

                results
                    .items
                    .into_iter()
                    .zip(results.scores)
                    .filter_map(|(file, score)| {
                        let path = prefixed_path(&scoped.prefix, file.relative_path(picker));
                        self.indexed_path(path).map(|path| FileHit {
                            path,
                            score: score.total,
                            frecency: file.total_frecency_score().max(0) as u32,
                        })
                    })
                    .collect::<Vec<_>>()
            })?);
        }
        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.frecency.cmp(&left.frecency))
                .then_with(|| left.path.cmp(&right.path))
        });
        hits.dedup_by(|left, right| left.path == right.path);
        hits.truncate(limit);
        Ok(hits)
    }
}

#[derive(Debug)]
struct SourceScanPlan {
    directories: Vec<ScanRoot>,
    files: Vec<String>,
}

#[derive(Debug)]
struct ScanRoot {
    path: String,
}

impl SourceScanPlan {
    fn new(root: &Path, source_roots: &[String]) -> Result<Self> {
        let mut directories = BTreeSet::new();
        let mut files = BTreeSet::new();
        for source_root in source_roots {
            match source_root_kind(root, source_root)? {
                Some(SourceRootKind::File) => {
                    files.insert(source_root.clone());
                }
                Some(SourceRootKind::Directory) => {
                    directories.insert(source_root.clone());
                }
                None => {}
            }
        }
        Ok(Self {
            directories: directories
                .into_iter()
                .map(|path| ScanRoot { path })
                .collect(),
            files: files.into_iter().collect(),
        })
    }
}

fn prefixed_path(prefix: &str, path: String) -> String {
    if prefix == "." {
        path
    } else if path.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}/{path}")
    }
}

impl SourceIndex for FffSourceIndex {
    fn resolve_partial_path(
        &self,
        entries: &[IndexedSource],
        path: &str,
    ) -> Option<(String, bool)> {
        if path.is_empty() || path.starts_with("match:") {
            return None;
        }

        let path = path.trim();
        if entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .is_ok()
        {
            return Some((path.to_string(), false));
        }

        let fff_matches = self
            .partial_path_candidates(path, 50)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .map(|hit| hit.path)
            .filter(|file| file.ends_with(path) || file.rsplit('/').next() == Some(path))
            .collect::<Vec<_>>();

        let matches = if fff_matches.is_empty() {
            entries
                .iter()
                .map(|entry| &entry.path)
                .filter(|file| file.ends_with(path) || file.rsplit('/').next() == Some(path))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            fff_matches
        };

        match matches.as_slice() {
            [] => None,
            [one] => Some((one.clone(), false)),
            many => Some((many[0].clone(), true)),
        }
    }

    fn observe(&self) -> Result<IndexSnapshot> {
        self.observation()
    }
}

#[derive(Debug)]
struct EmptySourceIndex;

impl SourceIndex for EmptySourceIndex {
    fn resolve_partial_path(
        &self,
        _entries: &[IndexedSource],
        _path: &str,
    ) -> Option<(String, bool)> {
        None
    }

    fn observe(&self) -> Result<IndexSnapshot> {
        Ok(IndexSnapshot {
            entries: Vec::new(),
            fingerprint: String::new(),
        })
    }
}

fn normalize_source_roots(source_roots: &[String]) -> Vec<String> {
    source_roots
        .iter()
        .map(|root| root.trim().trim_matches('/').to_string())
        .filter(|root| !root.is_empty())
        .collect()
}

fn fuzzy_score(value: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let value = value.to_lowercase();
    let query = query.to_lowercase();
    let mut value_chars = value.chars();
    let mut score = 0;
    for query_char in query.chars() {
        loop {
            match value_chars.next() {
                Some(value_char) if value_char == query_char => {
                    score += 1;
                    break;
                }
                Some(_) => {}
                None => return None,
            }
        }
    }
    Some(score)
}

fn explicit_file_fingerprint(root: &Path, path: &str) -> Result<String> {
    let metadata = source_metadata(root, path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    Ok(format!("{path}\0{}\0{}", metadata.len(), modified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;
    use tempfile::TempDir;

    #[test]
    fn source_index_scans_configured_roots_and_preserves_file_roots() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".github/workflows")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        fs::write(root.join(".github/workflows/ci.yml"), "name: CI\n").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
        fs::write(root.join("docs/ignored.md"), "# ignored\n").unwrap();

        reset_work_counts();
        let config = test_config(&["src", ".github/workflows", "Cargo.toml"], &[], true);
        let mut lifecycle = SourceIndexLifecycle::for_command(root, &config).unwrap();
        let catalog = lifecycle.observe().unwrap().into_catalog();

        let entries = catalog
            .entries()
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            vec![".github/workflows/ci.yml", "Cargo.toml", "src/lib.rs"]
        );
        assert_eq!(
            catalog.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        assert_eq!(
            catalog.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        assert_eq!(
            work_counts(),
            WorkCounts {
                fff_starts: 1,
                observations: 1,
            },
            "the command lifecycle should make one full observation"
        );
    }

    #[test]
    fn source_index_enumeration_preserves_vault_source_file_semantics() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        fs::write(root.join("src/nested/mod.rs"), "pub fn nested() {}\n").unwrap();
        fs::write(root.join("src/.hidden.rs"), "pub fn hidden() {}\n").unwrap();
        fs::write(root.join("src/excluded.rs"), "pub fn excluded() {}\n").unwrap();
        fs::write(root.join("src/ignored.rs"), "pub fn ignored() {}\n").unwrap();
        fs::write(root.join("src/.gitignore"), "ignored.rs\n").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
        fs::write(root.join("binary.bin"), [0_u8, 159, 146, 150]).unwrap();

        let config = test_config(
            &["src", "src/nested", "Cargo.toml", "binary.bin", "src"],
            &["src/excluded.rs"],
            true,
        );
        let mut lifecycle = SourceIndexLifecycle::for_command(root, &config).unwrap();
        let catalog = lifecycle.observe().unwrap().into_catalog();

        let paths = catalog
            .entries()
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "Cargo.toml",
                "src/ignored.rs",
                "src/lib.rs",
                "src/nested/mod.rs",
            ]
        );
    }

    #[test]
    fn command_lifecycle_keeps_one_stable_observation() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/one.rs"), "fn one() {}\n").unwrap();
        let config = test_config(&["src"], &[], true);

        reset_work_counts();
        let mut lifecycle = SourceIndexLifecycle::for_command(root, &config).unwrap();
        assert_eq!(observation_paths(&mut lifecycle), vec!["src/one.rs"]);
        fs::write(root.join("src/two.rs"), "fn two() {}\n").unwrap();
        assert_eq!(observation_paths(&mut lifecycle), vec!["src/one.rs"]);
        assert_eq!(work_counts().fff_starts, 1);
        assert_eq!(work_counts().observations, 1);
    }

    #[test]
    fn live_lifecycle_does_not_cache_observations() {
        let _live_test = lock_live_test();
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/one.rs"), "fn one() {}\n").unwrap();
        let config = test_config(&["src"], &[], true);
        let mut lifecycle = SourceIndexLifecycle::for_watch(root, &config).unwrap();

        reset_work_counts();
        let first = lifecycle.observe().unwrap();
        let second = lifecycle.observe().unwrap();

        assert_eq!(first.change(), SourceChange::Unchanged);
        assert_eq!(second.change(), SourceChange::Unchanged);
        assert_eq!(catalog_paths(&first.catalog), vec!["src/one.rs"]);
        assert_eq!(catalog_paths(&second.catalog), vec!["src/one.rs"]);
        assert_eq!(work_counts().observations, 2);
    }

    #[test]
    fn live_lifecycle_observes_add_modify_rename_and_delete() {
        let _live_test = lock_live_test();
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/one.rs"), "fn one() {}\n").unwrap();
        let config = test_config(&["src"], &[], true);

        reset_work_counts();
        let mut lifecycle = SourceIndexLifecycle::for_watch(root, &config).unwrap();
        let handle = lifecycle.handle();
        let initial = lifecycle.observe().unwrap();
        assert_eq!(initial.change(), SourceChange::Unchanged);
        assert_eq!(catalog_paths(&initial.catalog), vec!["src/one.rs"]);

        fs::write(root.join("src/two.rs"), "fn two() {}\n").unwrap();
        wait_for_paths(&handle, &["src/one.rs", "src/two.rs"]);
        let addition = wait_for_change(&mut lifecycle, "source addition");
        assert_eq!(
            catalog_paths(&addition.catalog),
            vec!["src/one.rs", "src/two.rs"]
        );

        fs::write(
            root.join("src/one.rs"),
            "fn one() { println!(\"changed\"); }\n",
        )
        .unwrap();
        let modification = wait_for_change(&mut lifecycle, "source modification");
        assert_eq!(
            catalog_paths(&modification.catalog),
            vec!["src/one.rs", "src/two.rs"]
        );

        fs::rename(root.join("src/two.rs"), root.join("src/three.rs")).unwrap();
        wait_for_paths(&handle, &["src/one.rs", "src/three.rs"]);
        let rename = wait_for_change(&mut lifecycle, "source rename");
        assert_eq!(
            catalog_paths(&rename.catalog),
            vec!["src/one.rs", "src/three.rs"]
        );

        fs::remove_file(root.join("src/three.rs")).unwrap();
        wait_for_paths(&handle, &["src/one.rs"]);
        let deletion = wait_for_change(&mut lifecycle, "source deletion");
        assert_eq!(catalog_paths(&deletion.catalog), vec!["src/one.rs"]);

        assert_eq!(work_counts().fff_starts, 1);
        assert!(
            work_counts().observations >= 5,
            "each watcher poll should make a fresh full observation"
        );
    }

    #[test]
    fn disabled_lifecycles_use_the_empty_adapter_without_starting_fff() {
        let temp = TempDir::new().unwrap();
        let config = test_config(&["src"], &[], false);

        reset_work_counts();
        let mut command = SourceIndexLifecycle::for_command(temp.path(), &config).unwrap();
        let mut watch = SourceIndexLifecycle::for_watch(temp.path(), &config).unwrap();

        assert!(!command.handle().is_enabled());
        assert!(!watch.handle().is_enabled());
        assert!(observation_paths(&mut command).is_empty());
        assert_eq!(watch.observe().unwrap().change(), SourceChange::Disabled);
        assert_eq!(work_counts().fff_starts, 0);
    }

    #[test]
    fn source_index_rejects_parent_traversing_source_roots() {
        let temp = TempDir::new().unwrap();
        let config = test_config(&["../outside"], &[], true);
        let error = SourceIndexLifecycle::for_command(temp.path(), &config)
            .expect_err("parent source root should fail");

        assert!(error.to_string().contains("parent-directory"));
    }

    #[cfg(unix)]
    #[test]
    fn source_index_rejects_symlinked_source_roots_and_files() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path().join("vault");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.rs"), "pub fn secret() {}\n").unwrap();
        symlink(&outside, root.join("src")).unwrap();

        let config = test_config(&["src"], &[], true);
        let error = SourceIndexLifecycle::for_command(&root, &config)
            .expect_err("symlinked source root should fail");
        assert!(error.to_string().contains("must not be a symlink"));

        fs::remove_file(root.join("src")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        symlink(outside.join("secret.rs"), root.join("src/secret.rs")).unwrap();

        let config = test_config(&["src/secret.rs"], &[], true);
        let error = SourceIndexLifecycle::for_command(&root, &config)
            .expect_err("symlinked source file root should fail");
        assert!(error.to_string().contains("must not be a symlink"));
    }

    fn test_config(roots: &[&str], exclude: &[&str], source_index: bool) -> Config {
        Config {
            source_roots: roots.iter().map(|root| (*root).to_string()).collect(),
            source_exclude: exclude
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
            source_index,
            ..Config::default()
        }
    }

    fn observation_paths(lifecycle: &mut SourceIndexLifecycle) -> Vec<String> {
        let observation = lifecycle.observe().unwrap();
        catalog_paths(&observation.catalog)
    }

    fn catalog_paths(catalog: &SourceCatalog) -> Vec<String> {
        catalog
            .entries()
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn paths(handle: &SourceIndexHandle) -> Vec<String> {
        handle
            .as_index()
            .observe()
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect()
    }

    fn wait_for_paths(handle: &SourceIndexHandle, expected: &[&str]) {
        let expected = expected
            .iter()
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let actual = paths(handle);
            if actual == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected:?}, got {actual:?}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_change(lifecycle: &mut SourceIndexLifecycle, label: &str) -> SourceObservation {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let observation = lifecycle.observe().unwrap();
            match observation.change() {
                SourceChange::Changed => return observation,
                SourceChange::Unchanged => {}
                SourceChange::Disabled => panic!("{label} used a disabled source index"),
            }
            assert!(Instant::now() < deadline, "timed out observing {label}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

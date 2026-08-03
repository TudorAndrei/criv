use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

#[cfg(test)]
use std::{cell::Cell, thread_local};

use fff_search::file_picker::FilePicker;
use fff_search::{
    FFFMode, FilePickerOptions, FuzzySearchOptions, GrepSearchOptions, PaginationArgs, QueryParser,
    SharedFilePicker, SharedFrecency, parse_grep_query,
};
use regex::Regex;

use crate::config::Config;
use crate::source_paths::{
    SourceRootKind, canonical_source_path, read_source_to_string, source_metadata, source_root_kind,
};
use crate::util::{GlobMatcher, is_text_file};
use crate::{CrivError, Result};

const SCAN_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) fff_starts: usize,
    pub(crate) catalog_traversals: usize,
    pub(crate) source_enumerations: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        fff_starts: 0,
        catalog_traversals: 0,
        source_enumerations: 0,
    }) };
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceGrepMode {
    Plain,
    Regex,
    Fuzzy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileHit {
    pub(crate) path: String,
    score: i32,
    frecency: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrepHit {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedSource {
    pub(crate) path: String,
    pub(crate) frecency: u32,
}

pub(crate) trait SourceIndex: std::fmt::Debug + Send + Sync {
    fn fuzzy_files(&self, query: &str, limit: usize) -> Result<Vec<FileHit>>;
    fn grep(&self, query: &str, mode: SourceGrepMode, paths: &[String]) -> Result<Vec<GrepHit>>;
    fn resolve_partial_path(&self, path: &str) -> Option<(String, bool)>;
    fn entries(&self) -> Result<Vec<IndexedSource>>;
}

#[derive(Debug, Clone)]
pub(crate) struct SourceIndexHandle {
    adapter: SourceIndexAdapter,
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

    pub(crate) fn disabled() -> Self {
        Self {
            adapter: SourceIndexAdapter::Empty(Arc::new(EmptySourceIndex)),
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        matches!(self.adapter, SourceIndexAdapter::Fff(_))
    }

    pub(crate) fn as_index(&self) -> &dyn SourceIndex {
        match &self.adapter {
            SourceIndexAdapter::Fff(index) => index.as_ref(),
            SourceIndexAdapter::Empty(index) => index.as_ref(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct OneShotSourceIndex {
    handle: SourceIndexHandle,
}

impl OneShotSourceIndex {
    pub(crate) fn new(root: &Path, config: &Config) -> Result<Self> {
        let handle = if config.source_index {
            SourceIndexHandle::enabled(Arc::new(FffSourceIndex::new(
                root,
                &config.source_roots,
                &config.source_exclude,
                SourceIndexLifetime::OneShot,
            )?))
        } else {
            SourceIndexHandle::disabled()
        };
        Ok(Self { handle })
    }

    pub(crate) fn handle(&self) -> SourceIndexHandle {
        self.handle.clone()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourceChange {
    Changed,
    Unchanged,
    Disabled,
}

#[derive(Debug)]
pub(crate) struct LiveSourceIndex {
    handle: SourceIndexHandle,
    index: Option<Arc<FffSourceIndex>>,
    fingerprint: Option<String>,
}

impl LiveSourceIndex {
    pub(crate) fn new(root: &Path, config: &Config) -> Result<Self> {
        let index = config
            .source_index
            .then(|| {
                FffSourceIndex::new(
                    root,
                    &config.source_roots,
                    &config.source_exclude,
                    SourceIndexLifetime::Live,
                )
                .map(Arc::new)
            })
            .transpose()?;
        let fingerprint = index
            .as_ref()
            .map(|index| index.source_fingerprint())
            .transpose()?;
        let handle = index
            .as_ref()
            .map_or_else(SourceIndexHandle::disabled, |index| {
                SourceIndexHandle::enabled(index.clone())
            });
        Ok(Self {
            handle,
            index,
            fingerprint,
        })
    }

    pub(crate) fn handle(&self) -> SourceIndexHandle {
        self.handle.clone()
    }

    pub(crate) fn observe_source_change(&mut self) -> Result<SourceChange> {
        let Some(index) = &self.index else {
            return Ok(SourceChange::Disabled);
        };
        let next = index.source_fingerprint()?;
        let changed = self
            .fingerprint
            .replace(next.clone())
            .is_some_and(|previous| previous != next);
        Ok(if changed {
            SourceChange::Changed
        } else {
            SourceChange::Unchanged
        })
    }
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
    source_files_cache: Option<OnceLock<Vec<String>>>,
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

            if !picker.wait_for_scan(SCAN_TIMEOUT) {
                return Err(CrivError::new("timed out scanning source files with fff"));
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
            source_files_cache: (lifetime == SourceIndexLifetime::OneShot).then(OnceLock::new),
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

    fn source_files(&self) -> Result<Vec<String>> {
        if let Some(cache) = &self.source_files_cache {
            if let Some(cached) = cache.get() {
                return Ok(cached.clone());
            }
            let files = self.collect_source_files_now()?;
            return Ok(cache.get_or_init(|| files).clone());
        }
        self.collect_source_files_now()
    }

    fn collect_source_files_now(&self) -> Result<Vec<String>> {
        #[cfg(test)]
        record_work(|counts| counts.source_enumerations += 1);

        let mut files = BTreeSet::new();
        for scoped in &self.pickers {
            files.extend(self.with_picker(scoped, |picker| {
                picker
                    .get_files()
                    .iter()
                    .filter(|file| !file.is_binary() && !file.is_deleted())
                    .filter_map(|file| {
                        let path = prefixed_path(&scoped.prefix, file.relative_path(picker));
                        self.indexed_path(path)
                    })
                    .collect::<Vec<_>>()
            })?);
        }
        files.extend(
            self.explicit_files
                .iter()
                .filter(|path| is_text_file(&self.root.join(path)).unwrap_or(false))
                .filter_map(|path| self.indexed_path(path.clone())),
        );
        Ok(files.into_iter().collect())
    }

    fn source_fingerprint(&self) -> Result<String> {
        let mut rows = Vec::new();
        for scoped in &self.pickers {
            rows.extend(self.with_picker(scoped, |picker| {
                picker
                    .get_files()
                    .iter()
                    .filter(|file| !file.is_binary() && !file.is_deleted())
                    .filter_map(|file| {
                        let path = prefixed_path(&scoped.prefix, file.relative_path(picker));
                        self.indexed_path(path.clone())
                            .is_some()
                            .then_some(format!("{path}\0{}\0{}", file.size, file.modified))
                    })
                    .collect::<Vec<_>>()
            })?);
        }
        rows.extend(
            self.explicit_files
                .iter()
                .filter(|path| self.indexed_path((*path).clone()).is_some())
                .filter_map(|path| explicit_file_fingerprint(&self.root, path).ok()),
        );
        rows.sort();
        rows.dedup();
        Ok(blake3::hash(rows.join("\n").as_bytes())
            .to_hex()
            .to_string())
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

    fn grep_explicit_files(
        &self,
        query: &str,
        mode: SourceGrepMode,
        paths: Option<&GlobMatcher>,
        matcher: Option<&Regex>,
    ) -> Vec<GrepHit> {
        let plain_query = query.to_lowercase();
        let mut rows = Vec::new();
        for path in self
            .explicit_files
            .iter()
            .filter(|path| self.source_path_allowed(path) && path_allowed(path, paths))
        {
            let Ok(contents) = read_source_to_string(&self.root, path) else {
                continue;
            };
            for (index, line) in contents.lines().enumerate() {
                let matched = match mode {
                    SourceGrepMode::Plain => line.to_lowercase().contains(&plain_query),
                    SourceGrepMode::Regex => matcher.is_some_and(|matcher| matcher.is_match(line)),
                    SourceGrepMode::Fuzzy => fuzzy_score(line, query).is_some(),
                };
                if matched {
                    rows.push(GrepHit {
                        path: path.clone(),
                        line: index + 1,
                        text: line.trim().to_string(),
                    });
                }
            }
        }
        rows
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
    fn fuzzy_files(&self, query: &str, limit: usize) -> Result<Vec<FileHit>> {
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

    fn grep(&self, query: &str, mode: SourceGrepMode, paths: &[String]) -> Result<Vec<GrepHit>> {
        let regex_matcher = regex_matcher(query, mode)?;
        let path_matcher = (!paths.is_empty()).then(|| GlobMatcher::from_valid_patterns(paths));
        let mut rows =
            self.grep_explicit_files(query, mode, path_matcher.as_ref(), regex_matcher.as_ref());
        for scoped in &self.pickers {
            rows.extend(self.with_picker(scoped, |picker| {
                let grep_query = match mode {
                    SourceGrepMode::Plain => query.to_lowercase(),
                    SourceGrepMode::Regex | SourceGrepMode::Fuzzy => query.to_string(),
                };
                let parsed = parse_grep_query(&grep_query);
                let results = picker.grep(
                    &parsed,
                    &GrepSearchOptions {
                        mode: match mode {
                            SourceGrepMode::Plain => fff_search::GrepMode::PlainText,
                            SourceGrepMode::Regex => fff_search::GrepMode::Regex,
                            SourceGrepMode::Fuzzy => fff_search::GrepMode::Fuzzy,
                        },
                        page_limit: 10_000,
                        trim_whitespace: true,
                        ..Default::default()
                    },
                );

                let mut scoped_rows = Vec::new();
                for matched in results.matches {
                    let Some(file) = results.files.get(matched.file_index) else {
                        continue;
                    };
                    let path = prefixed_path(&scoped.prefix, file.relative_path(picker));
                    if self.indexed_path(path.clone()).is_none()
                        || !path_allowed(&path, path_matcher.as_ref())
                    {
                        continue;
                    }
                    scoped_rows.push(GrepHit {
                        path,
                        line: matched.line_number as usize,
                        text: matched.line_content.trim().to_string(),
                    });
                }
                scoped_rows
            })?);
        }
        rows.sort_by(|left, right| {
            (&left.path, left.line, &left.text).cmp(&(&right.path, right.line, &right.text))
        });
        rows.dedup();
        Ok(rows)
    }

    fn resolve_partial_path(&self, path: &str) -> Option<(String, bool)> {
        if path.is_empty() || path.starts_with("match:") {
            return None;
        }

        let path = path.trim();
        let source_files = self.source_files().ok()?;
        if source_files.iter().any(|source_file| source_file == path) {
            return Some((path.to_string(), false));
        }

        let fff_matches = self
            .fuzzy_files(path, 50)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .map(|hit| hit.path)
            .filter(|file| file.ends_with(path) || file.rsplit('/').next() == Some(path))
            .collect::<Vec<_>>();

        let matches = if fff_matches.is_empty() {
            source_files
                .iter()
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

    fn entries(&self) -> Result<Vec<IndexedSource>> {
        #[cfg(test)]
        record_work(|counts| counts.catalog_traversals += 1);

        let mut frecency_by_path = BTreeMap::new();
        for scoped in &self.pickers {
            frecency_by_path.extend(self.with_picker(scoped, |picker| {
                picker
                    .get_files()
                    .iter()
                    .filter_map(|file| {
                        let path = prefixed_path(&scoped.prefix, file.relative_path(picker));
                        (!file.is_binary()
                            && !file.is_deleted()
                            && self.indexed_path(path.clone()).is_some())
                        .then_some((path, file.total_frecency_score().max(0) as u32))
                    })
                    .collect::<BTreeMap<_, _>>()
            })?);
        }
        let mut entries = self
            .source_files()?
            .into_iter()
            .map(|path| IndexedSource {
                frecency: frecency_by_path.get(&path).copied().unwrap_or(0),
                path,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }
}

#[derive(Debug)]
struct EmptySourceIndex;

impl SourceIndex for EmptySourceIndex {
    fn fuzzy_files(&self, _query: &str, _limit: usize) -> Result<Vec<FileHit>> {
        Ok(Vec::new())
    }

    fn grep(&self, _query: &str, _mode: SourceGrepMode, _paths: &[String]) -> Result<Vec<GrepHit>> {
        Ok(Vec::new())
    }

    fn resolve_partial_path(&self, _path: &str) -> Option<(String, bool)> {
        None
    }

    fn entries(&self) -> Result<Vec<IndexedSource>> {
        Ok(Vec::new())
    }
}

fn path_allowed(path: &str, matcher: Option<&GlobMatcher>) -> bool {
    matcher.is_none_or(|matcher| matcher.is_match(path))
}

fn regex_matcher(query: &str, mode: SourceGrepMode) -> Result<Option<Regex>> {
    match mode {
        SourceGrepMode::Regex => Regex::new(query)
            .map(Some)
            .map_err(|err| CrivError::new(format!("invalid regex grep query `{query}`: {err}"))),
        SourceGrepMode::Plain | SourceGrepMode::Fuzzy => Ok(None),
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
        let lifecycle = OneShotSourceIndex::new(root, &config).unwrap();
        let handle = lifecycle.handle();
        let index = handle.as_index();

        let entries = index
            .entries()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            vec![".github/workflows/ci.yml", "Cargo.toml", "src/lib.rs"]
        );
        assert!(
            index
                .fuzzy_files("Cargo", 10)
                .unwrap()
                .iter()
                .any(|hit| hit.path == "Cargo.toml")
        );
        assert!(
            index.fuzzy_files("", 2).unwrap().len() <= 2,
            "file search should honor requested result limits"
        );
        assert!(
            index
                .grep("package", SourceGrepMode::Plain, &[])
                .unwrap()
                .iter()
                .any(|hit| hit.path == "Cargo.toml" && hit.line == 1)
        );
        assert!(
            index
                .grep("pkg", SourceGrepMode::Fuzzy, &["Cargo.toml".into()])
                .unwrap()
                .iter()
                .any(|hit| hit.path == "Cargo.toml" && hit.line == 1)
        );
        assert_eq!(
            index.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        assert_eq!(
            index.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        let error = index
            .grep("[", SourceGrepMode::Regex, &[])
            .expect_err("invalid regex should fail");
        assert!(error.to_string().contains("invalid regex grep query"));
        assert_eq!(
            work_counts(),
            WorkCounts {
                fff_starts: 1,
                catalog_traversals: 1,
                source_enumerations: 1,
            },
            "the one-shot index should traverse the catalog once and reuse its enumeration"
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
        let lifecycle = OneShotSourceIndex::new(root, &config).unwrap();
        let handle = lifecycle.handle();
        let index = handle.as_index();

        let paths = index
            .entries()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
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
    fn one_shot_lifecycle_keeps_one_stable_enumeration() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/one.rs"), "fn one() {}\n").unwrap();
        let config = test_config(&["src"], &[], true);

        reset_work_counts();
        let lifecycle = OneShotSourceIndex::new(root, &config).unwrap();
        let handle = lifecycle.handle();
        assert_eq!(paths(&handle), vec!["src/one.rs"]);
        fs::write(root.join("src/two.rs"), "fn two() {}\n").unwrap();
        assert_eq!(paths(&handle), vec!["src/one.rs"]);
        assert_eq!(work_counts().fff_starts, 1);
        assert_eq!(work_counts().source_enumerations, 1);
    }

    #[test]
    fn live_lifecycle_observes_add_modify_rename_and_delete() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/one.rs"), "fn one() {}\n").unwrap();
        let config = test_config(&["src"], &[], true);

        reset_work_counts();
        let mut lifecycle = LiveSourceIndex::new(root, &config).unwrap();
        let handle = lifecycle.handle();
        assert_eq!(paths(&handle), vec!["src/one.rs"]);
        assert_eq!(
            lifecycle.observe_source_change().unwrap(),
            SourceChange::Unchanged
        );

        fs::write(root.join("src/two.rs"), "fn two() {}\n").unwrap();
        wait_for_paths(&handle, &["src/one.rs", "src/two.rs"]);
        wait_for_change(&mut lifecycle, "source addition");

        fs::write(
            root.join("src/one.rs"),
            "fn one() { println!(\"changed\"); }\n",
        )
        .unwrap();
        wait_for_change(&mut lifecycle, "source modification");

        fs::rename(root.join("src/two.rs"), root.join("src/three.rs")).unwrap();
        wait_for_paths(&handle, &["src/one.rs", "src/three.rs"]);
        wait_for_change(&mut lifecycle, "source rename");

        fs::remove_file(root.join("src/three.rs")).unwrap();
        wait_for_paths(&handle, &["src/one.rs"]);
        wait_for_change(&mut lifecycle, "source deletion");

        assert_eq!(work_counts().fff_starts, 1);
    }

    #[test]
    fn disabled_lifecycles_use_the_empty_adapter_without_starting_fff() {
        let temp = TempDir::new().unwrap();
        let config = test_config(&["src"], &[], false);

        reset_work_counts();
        let one_shot = OneShotSourceIndex::new(temp.path(), &config).unwrap();
        let mut live = LiveSourceIndex::new(temp.path(), &config).unwrap();

        assert!(!one_shot.handle().is_enabled());
        assert!(!live.handle().is_enabled());
        assert!(paths(&one_shot.handle()).is_empty());
        assert_eq!(
            live.observe_source_change().unwrap(),
            SourceChange::Disabled
        );
        assert_eq!(work_counts().fff_starts, 0);
    }

    #[test]
    fn source_index_rejects_parent_traversing_source_roots() {
        let temp = TempDir::new().unwrap();
        let config = test_config(&["../outside"], &[], true);
        let error = OneShotSourceIndex::new(temp.path(), &config)
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
        let error =
            OneShotSourceIndex::new(&root, &config).expect_err("symlinked source root should fail");
        assert!(error.to_string().contains("must not be a symlink"));

        fs::remove_file(root.join("src")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        symlink(outside.join("secret.rs"), root.join("src/secret.rs")).unwrap();

        let config = test_config(&["src/secret.rs"], &[], true);
        let error = OneShotSourceIndex::new(&root, &config)
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

    fn paths(handle: &SourceIndexHandle) -> Vec<String> {
        handle
            .as_index()
            .entries()
            .unwrap()
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

    fn wait_for_change(lifecycle: &mut LiveSourceIndex, label: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match lifecycle.observe_source_change().unwrap() {
                SourceChange::Changed => return,
                SourceChange::Unchanged => {}
                SourceChange::Disabled => panic!("{label} used a disabled source index"),
            }
            assert!(Instant::now() < deadline, "timed out observing {label}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

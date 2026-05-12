use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use fff_search::file_picker::FilePicker;
use fff_search::{
    FFFMode, FilePickerOptions, FuzzySearchOptions, GrepSearchOptions, PaginationArgs, QueryParser,
    SharedFilePicker, SharedFrecency, parse_grep_query,
};

use crate::util::glob_matches;
use crate::{CrivError, Result};

const SCAN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceGrepMode {
    Plain,
    Regex,
    Fuzzy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileHit {
    pub(crate) path: String,
    pub(crate) score: i32,
    pub(crate) frecency: u32,
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

pub(crate) trait SourceIndex: std::fmt::Debug {
    fn fuzzy_files(&self, query: &str, limit: usize) -> Result<Vec<FileHit>>;
    fn grep(&self, query: &str, mode: SourceGrepMode, paths: &[String]) -> Result<Vec<GrepHit>>;
    fn resolve_partial_path(&self, path: &str) -> Option<(String, bool)>;
    fn entries(&self) -> Result<Vec<IndexedSource>>;
}

#[derive(Debug)]
pub(crate) struct FffSourceIndex {
    source_files: Vec<String>,
    source_set: BTreeSet<String>,
    picker: SharedFilePicker,
    _frecency: SharedFrecency,
}

impl FffSourceIndex {
    pub(crate) fn new(root: &Path, source_files: &[String]) -> Result<Self> {
        let picker = SharedFilePicker::default();
        let frecency = SharedFrecency::default();
        FilePicker::new_with_shared_state(
            picker.clone(),
            frecency.clone(),
            FilePickerOptions {
                base_path: root.to_string_lossy().to_string(),
                mode: FFFMode::Ai,
                watch: false,
                ..Default::default()
            },
        )
        .map_err(|err| CrivError::new(format!("failed to start fff source index: {err}")))?;

        if !picker.wait_for_scan(SCAN_TIMEOUT) {
            return Err(CrivError::new("timed out scanning source files with fff"));
        }

        Ok(Self {
            source_files: source_files.to_vec(),
            source_set: source_files.iter().cloned().collect(),
            picker,
            _frecency: frecency,
        })
    }

    fn with_picker<T>(&self, f: impl FnOnce(&FilePicker) -> T) -> Result<T> {
        let guard = self
            .picker
            .read()
            .map_err(|err| CrivError::new(format!("failed to read fff source index: {err}")))?;
        let picker = guard
            .as_ref()
            .ok_or_else(|| CrivError::new("fff source index is not initialized"))?;
        Ok(f(picker))
    }

    fn indexed_path(&self, path: String) -> Option<String> {
        self.source_set.contains(&path).then_some(path)
    }
}

impl SourceIndex for FffSourceIndex {
    fn fuzzy_files(&self, query: &str, limit: usize) -> Result<Vec<FileHit>> {
        self.with_picker(|picker| {
            let parser = QueryParser::default();
            let query = parser.parse(query);
            let results = picker.fuzzy_search(
                &query,
                None,
                FuzzySearchOptions {
                    max_threads: 0,
                    project_path: None,
                    current_file: None,
                    pagination: PaginationArgs {
                        offset: 0,
                        limit: limit.max(self.source_files.len()),
                    },
                    ..Default::default()
                },
            );

            results
                .items
                .into_iter()
                .zip(results.scores)
                .filter_map(|(file, score)| {
                    self.indexed_path(file.relative_path(picker))
                        .map(|path| FileHit {
                            path,
                            score: score.total,
                            frecency: file.total_frecency_score().max(0) as u32,
                        })
                })
                .take(limit)
                .collect::<Vec<_>>()
        })
    }

    fn grep(&self, query: &str, mode: SourceGrepMode, paths: &[String]) -> Result<Vec<GrepHit>> {
        self.with_picker(|picker| {
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

            let mut rows = Vec::new();
            for matched in results.matches {
                let Some(file) = results.files.get(matched.file_index) else {
                    continue;
                };
                let path = file.relative_path(picker);
                if !self.source_set.contains(&path) || !path_allowed(&path, paths) {
                    continue;
                }
                rows.push(GrepHit {
                    path,
                    line: matched.line_number as usize,
                    text: matched.line_content.trim().to_string(),
                });
            }
            rows
        })
    }

    fn resolve_partial_path(&self, path: &str) -> Option<(String, bool)> {
        if path.is_empty() || path.starts_with("match:") {
            return None;
        }

        let path = path.trim();
        if self.source_set.contains(path) {
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
            self.source_files
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
        let frecency_by_path = self.with_picker(|picker| {
            picker
                .get_files()
                .iter()
                .filter_map(|file| {
                    let path = file.relative_path(picker);
                    self.source_set
                        .contains(&path)
                        .then_some((path, file.total_frecency_score().max(0) as u32))
                })
                .collect::<BTreeMap<_, _>>()
        })?;
        let mut entries = self
            .source_files
            .iter()
            .map(|path| IndexedSource {
                path: path.clone(),
                frecency: frecency_by_path.get(path).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }
}

fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| glob_matches(pattern, path))
}

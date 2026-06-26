use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use fff_search::file_picker::FilePicker;
use fff_search::{
    FFFMode, FilePickerOptions, FuzzySearchOptions, GrepSearchOptions, PaginationArgs, QueryParser,
    SharedFilePicker, SharedFrecency, parse_grep_query,
};
use regex::Regex;

use crate::source_paths::{
    SourceRootKind, canonical_source_path, read_source_to_string, source_metadata, source_root_kind,
};
use crate::util::{GlobMatcher, glob_matches};
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
    fn source_fingerprint(&self) -> Result<String>;
}

#[derive(Debug)]
pub(crate) struct FffSourceIndex {
    root: PathBuf,
    source_roots: Vec<String>,
    source_excludes: GlobMatcher,
    pickers: Vec<ScopedPicker>,
    explicit_files: Vec<String>,
}

#[derive(Debug)]
struct ScopedPicker {
    prefix: String,
    picker: SharedFilePicker,
    _frecency: SharedFrecency,
}

impl FffSourceIndex {
    pub(crate) fn new(
        root: &Path,
        source_roots: &[String],
        source_exclude: &[String],
        watch: bool,
    ) -> Result<Self> {
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
                .filter_map(|path| self.indexed_path(path.clone())),
        );
        Ok(files.into_iter().collect())
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
        paths: &[String],
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

    #[cfg(test)]
    fn scanned_roots(&self) -> Vec<String> {
        self.pickers
            .iter()
            .map(|picker| picker.prefix.clone())
            .collect()
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
                        pagination: PaginationArgs {
                            offset: 0,
                            limit: limit.max(picker.get_files().len()),
                        },
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
        let mut rows = self.grep_explicit_files(query, mode, paths, regex_matcher.as_ref());
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
                    if self.indexed_path(path.clone()).is_none() || !path_allowed(&path, paths) {
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
}

fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| glob_matches(pattern, path))
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

        let index = FffSourceIndex::new(
            root,
            &[
                "src".into(),
                ".github/workflows".into(),
                "Cargo.toml".into(),
            ],
            &[],
            false,
        )
        .unwrap();

        assert_eq!(index.scanned_roots(), vec![".github/workflows", "src"]);
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
        let error = index
            .grep("[", SourceGrepMode::Regex, &[])
            .expect_err("invalid regex should fail");
        assert!(error.to_string().contains("invalid regex grep query"));
    }

    #[test]
    fn source_index_rejects_parent_traversing_source_roots() {
        let temp = TempDir::new().unwrap();
        let error = FffSourceIndex::new(temp.path(), &["../outside".into()], &[], false)
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

        let error = FffSourceIndex::new(&root, &["src".into()], &[], false)
            .expect_err("symlinked source root should fail");
        assert!(error.to_string().contains("must not be a symlink"));

        fs::remove_file(root.join("src")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        symlink(outside.join("secret.rs"), root.join("src/secret.rs")).unwrap();

        let error = FffSourceIndex::new(&root, &["src/secret.rs".into()], &[], false)
            .expect_err("symlinked source file root should fail");
        assert!(error.to_string().contains("must not be a symlink"));
    }
}

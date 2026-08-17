use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use crate::Result;
use crate::config::Config;
use crate::discovery::discover_source_candidates;

use super::graph::SourceGraphBuild;

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    pub(crate) discovery_scans: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        discovery_scans: 0,
    }) };
}

#[cfg(test)]
static LIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_live_test() -> MutexGuard<'static, ()> {
    LIVE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

#[cfg(test)]
fn record_scan() {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.discovery_scans += 1;
        counts.set(next);
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedSource {
    pub(crate) path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceCatalog {
    entries: Arc<[IndexedSource]>,
    paths: Arc<[String]>,
}

impl SourceCatalog {
    pub(crate) fn disabled() -> Self {
        Self {
            entries: Arc::from([]),
            paths: Arc::from([]),
        }
    }

    fn enabled(paths: Vec<String>) -> Self {
        let entries = paths
            .iter()
            .cloned()
            .map(|path| IndexedSource { path })
            .collect::<Vec<_>>();
        Self {
            entries: entries.into(),
            paths: paths.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn discover(root: &Path, config: &Config) -> Result<Self> {
        Ok(SourceBuild::build_incremental(root, config, None)?.catalog)
    }

    pub(crate) fn entries(&self) -> &[IndexedSource] {
        &self.entries
    }

    pub(crate) fn paths(&self) -> &[String] {
        &self.paths
    }

    pub(crate) fn resolve_partial_path(&self, query: &str) -> Option<(String, bool)> {
        let query = query.trim();
        if query.is_empty() || query.starts_with("match:") {
            return None;
        }
        if self
            .paths
            .binary_search_by(|path| path.as_str().cmp(query))
            .is_ok()
        {
            return Some((query.to_string(), false));
        }

        let matches = self
            .paths
            .iter()
            .filter(|path| {
                path.strip_suffix(query)
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
                    || path.rsplit('/').next() == Some(query)
            })
            .cloned()
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => None,
            [only] => Some((only.clone(), false)),
            [first, ..] => Some((first.clone(), true)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceBuild {
    catalog: SourceCatalog,
    graph: SourceGraphBuild,
}

impl SourceBuild {
    pub(crate) fn build_incremental(
        root: &Path,
        config: &Config,
        previous: Option<&SourceGraphBuild>,
    ) -> Result<Self> {
        if !config.source_index {
            return Ok(Self::disabled());
        }
        #[cfg(test)]
        record_scan();
        let candidates = discover_source_candidates(root, config)?;
        let graph = SourceGraphBuild::build_incremental(root, &candidates, previous)?;
        let catalog = SourceCatalog::enabled(graph.paths());
        Ok(Self { catalog, graph })
    }

    pub(crate) fn disabled() -> Self {
        Self {
            catalog: SourceCatalog::disabled(),
            graph: SourceGraphBuild::disabled(),
        }
    }

    pub(crate) fn reused(&self) -> Self {
        Self {
            catalog: self.catalog.clone(),
            graph: self.graph.reused(),
        }
    }

    #[cfg(test)]
    pub(crate) fn catalog(&self) -> &SourceCatalog {
        &self.catalog
    }

    pub(crate) fn into_parts(self) -> (SourceCatalog, SourceGraphBuild) {
        (self.catalog, self.graph)
    }

    pub(crate) fn from_parts(catalog: SourceCatalog, graph: SourceGraphBuild) -> Self {
        Self { catalog, graph }
    }

    pub(crate) fn publish(mut self, root: &Path) -> Result<Self> {
        self.graph = self.graph.publish(root)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, Config) {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("src/nested")).unwrap();
        fs::write(temp.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        fs::write(
            temp.path().join("src/nested/lib.rs"),
            "pub fn nested() {}\n",
        )
        .unwrap();
        (
            temp,
            Config {
                source_roots: vec!["src".into()],
                source_exclude: vec![],
                ..Config::default()
            },
        )
    }

    #[test]
    fn catalog_uses_exact_suffix_basename_and_stable_ambiguity() {
        let (temp, config) = fixture();
        let catalog = SourceCatalog::discover(temp.path(), &config).unwrap();

        assert_eq!(
            catalog.resolve_partial_path("src/lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        assert_eq!(
            catalog.resolve_partial_path("nested/lib.rs"),
            Some(("src/nested/lib.rs".into(), false))
        );
        assert_eq!(
            catalog.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), true))
        );
    }

    #[test]
    fn repeated_discovery_observes_new_paths() {
        let (temp, config) = fixture();
        let first = SourceCatalog::discover(temp.path(), &config).unwrap();
        fs::write(temp.path().join("src/new.rs"), "pub fn new() {}\n").unwrap();
        let second = SourceCatalog::discover(temp.path(), &config).unwrap();
        assert_ne!(first.paths(), second.paths());
    }

    #[test]
    fn source_build_reads_each_candidate_once_and_shares_the_selected_set() {
        let (temp, config) = fixture();
        fs::write(temp.path().join("src/binary.rs"), b"\0binary").unwrap();
        super::super::graph::reset_work_counts();

        let build = SourceBuild::build_incremental(temp.path(), &config, None).unwrap();

        assert_eq!(super::super::graph::work_counts().source_reads, 3);
        assert_eq!(
            build.catalog().paths(),
            &["src/lib.rs".to_string(), "src/nested/lib.rs".to_string()]
        );
        assert_eq!(build.catalog().paths(), build.graph.paths());
    }
}

//! Source discovery, indexing, parsing, and graph construction.

mod catalog;
mod graph;
mod paths;

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use crate::Result;
use crate::config::Config;
use crate::discovery::discover_source_candidates;
use crate::repository::RepositoryFiles;

pub use graph::{
    DirectiveKind, Import, Language, ModuleRelationshipRole, Relationship, RelationshipKind,
    RelationshipTarget, SourceFile, SourceGraph, Symbol, SymbolKind,
};
#[cfg(test)]
pub(crate) use graph::{
    WorkCounts as GraphWorkCounts, reset_work_counts as reset_graph_work_counts,
    work_counts as graph_work_counts,
};
pub use paths::read_source_to_string_from;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSource {
    pub(crate) path: String,
}

/// One complete Source observation and its graph-cache lifecycle.
#[derive(Debug, Clone)]
pub struct SourceState {
    catalog: catalog::SourceCatalog,
    graph: graph::SourceGraphBuild,
}

impl SourceState {
    #[cfg(test)]
    pub(crate) fn refresh(root: &Path, config: &Config, previous: Option<&Self>) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::refresh_from(&files, config, previous)
    }

    pub(crate) fn refresh_from(
        files: &RepositoryFiles,
        config: &Config,
        previous: Option<&Self>,
    ) -> Result<Self> {
        if !config.source_index {
            return Ok(Self::disabled());
        }

        let root = files.root();
        #[cfg(test)]
        record_scan();
        let candidates = discover_source_candidates(root, config)?;
        let cached = previous
            .is_none()
            .then(|| graph::load_cached_from(files))
            .flatten();
        let previous_graph = previous.map(|state| &state.graph).or(cached.as_ref());
        let graph =
            graph::SourceGraphBuild::build_incremental_from(files, &candidates, previous_graph)?
                .publish_from(files)?;
        let catalog = catalog::SourceCatalog::enabled(graph.paths());
        Ok(Self { catalog, graph })
    }

    pub(crate) fn disabled() -> Self {
        Self {
            catalog: catalog::SourceCatalog::disabled(),
            graph: graph::SourceGraphBuild::disabled(),
        }
    }

    pub(crate) fn reuse_for_docs(&self) -> Self {
        Self {
            catalog: self.catalog.clone(),
            graph: self.graph.reused(),
        }
    }

    pub(crate) fn paths(&self) -> &[String] {
        self.catalog.paths()
    }

    pub(crate) fn entries(&self) -> &[IndexedSource] {
        self.catalog.entries()
    }

    pub(crate) const fn graph(&self) -> &SourceGraph {
        self.graph.graph()
    }

    pub(crate) fn changed_files(&self) -> &[String] {
        self.graph.graph().changed_files()
    }

    pub(crate) fn resolve_partial_path(&self, query: &str) -> Option<(String, bool)> {
        self.catalog.resolve_partial_path(query)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct IndexWorkCounts {
    pub(crate) discovery_scans: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<IndexWorkCounts> = const { Cell::new(IndexWorkCounts {
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
pub(crate) fn reset_index_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(IndexWorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn index_work_counts() -> IndexWorkCounts {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

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
    fn state_resolves_exact_suffix_basename_and_stable_ambiguity() {
        let (temp, config) = fixture();
        let state = SourceState::refresh(temp.path(), &config, None).unwrap();

        assert_eq!(
            state.resolve_partial_path("src/lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        assert_eq!(
            state.resolve_partial_path("nested/lib.rs"),
            Some(("src/nested/lib.rs".into(), false))
        );
        assert_eq!(
            state.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), true))
        );
    }

    #[test]
    fn repeated_refresh_observes_new_paths() {
        let (temp, config) = fixture();
        let first = SourceState::refresh(temp.path(), &config, None).unwrap();
        fs::write(temp.path().join("src/new.rs"), "pub fn new() {}\n").unwrap();
        let second = SourceState::refresh(temp.path(), &config, Some(&first)).unwrap();

        assert_ne!(first.paths(), second.paths());
    }

    #[test]
    fn state_reads_each_candidate_once_and_owns_one_selected_set() {
        let (temp, config) = fixture();
        fs::write(temp.path().join("src/binary.rs"), b"\0binary").unwrap();
        graph::reset_work_counts();

        let state = SourceState::refresh(temp.path(), &config, None).unwrap();

        assert_eq!(graph::work_counts().source_reads, 3);
        assert_eq!(
            state.paths(),
            &["src/lib.rs".to_string(), "src/nested/lib.rs".to_string()]
        );
        assert_eq!(
            state.paths(),
            state.graph().files.keys().cloned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn docs_reuse_clears_changes_without_source_work() {
        let (temp, config) = fixture();
        let state = SourceState::refresh(temp.path(), &config, None).unwrap();
        assert!(!state.changed_files().is_empty());
        reset_index_work_counts();
        graph::reset_work_counts();

        let reused = state.reuse_for_docs();

        assert_eq!(reused.paths(), state.paths());
        assert!(reused.changed_files().is_empty());
        assert_eq!(index_work_counts(), IndexWorkCounts::default());
        assert_eq!(graph::work_counts(), graph::WorkCounts::default());
    }

    #[test]
    fn disabled_state_does_no_source_work() {
        let (temp, mut config) = fixture();
        config.source_index = false;
        reset_index_work_counts();
        graph::reset_work_counts();

        let state = SourceState::refresh(temp.path(), &config, None).unwrap();

        assert!(state.paths().is_empty());
        assert!(state.entries().is_empty());
        assert!(state.graph().files.is_empty());
        assert_eq!(index_work_counts(), IndexWorkCounts::default());
        assert_eq!(graph::work_counts(), graph::WorkCounts::default());
    }
}

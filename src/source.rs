//! Source discovery, indexing, parsing, and graph construction.

mod catalog;
mod graph;
mod paths;

#[cfg(test)]
use std::cell::Cell;

use crate::Result;
use crate::config::Config;
use crate::discovery::{SourceScope, discover_source_candidates};
use crate::repository::RepositoryFiles;

pub use graph::{
    DirectiveKind, EdgeOrigin, Import, Language, ModuleRelationshipRole, Relationship,
    RelationshipKind, RelationshipTarget, SourceFile, SourceGraph, Symbol, SymbolId, SymbolKind,
};
#[cfg(test)]
pub(crate) use graph::{
    WorkCounts as GraphWorkCounts, reset_work_counts as reset_graph_work_counts,
    work_counts as graph_work_counts,
};
pub use paths::read_source_to_string_from;

/// One complete Source observation and its graph-cache lifecycle.
#[derive(Debug, Clone)]
pub struct SourceState {
    catalog: catalog::SourceCatalog,
    graph: graph::SourceGraphBuild,
}

impl SourceState {
    pub(crate) fn refresh(
        files: &RepositoryFiles,
        config: &Config,
        previous: Option<&Self>,
    ) -> Result<Self> {
        let scope = SourceScope::new(config)?;
        if !scope.is_enabled() {
            return Ok(Self::disabled());
        }

        #[cfg(test)]
        record_scan();
        let candidates = discover_source_candidates(files, &scope)?;
        let cached = match previous {
            Some(_) => None,
            None => graph::load_cached(files)?,
        };
        let previous_graph = previous.map(|state| &state.graph).or(cached.as_ref());
        let graph =
            graph::SourceGraphBuild::build(files, &candidates, previous_graph)?.publish(files)?;
        let catalog = catalog::SourceCatalog::new(graph.paths());
        Ok(Self { catalog, graph })
    }

    pub(crate) fn disabled() -> Self {
        Self {
            catalog: catalog::SourceCatalog::new(Vec::new()),
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
        next.discovery_scans = next.discovery_scans.saturating_add(1);
        counts.set(next);
    });
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

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

    fn refresh(
        root: &Path,
        config: &Config,
        previous: Option<&SourceState>,
    ) -> Result<SourceState> {
        SourceState::refresh(&RepositoryFiles::open(root)?, config, previous)
    }

    #[test]
    fn repeated_refresh_observes_new_paths() {
        let (temp, config) = fixture();
        let first = refresh(temp.path(), &config, None).unwrap();
        fs::write(temp.path().join("src/new.rs"), "pub fn new() {}\n").unwrap();
        let second = refresh(temp.path(), &config, Some(&first)).unwrap();

        assert_ne!(first.paths(), second.paths());
    }

    #[test]
    fn state_reads_each_candidate_once_and_owns_one_selected_set() {
        let (temp, config) = fixture();
        fs::write(temp.path().join("src/binary.rs"), b"\0binary").unwrap();
        graph::reset_work_counts();

        let state = refresh(temp.path(), &config, None).unwrap();

        assert_eq!(graph::work_counts().source_reads, 3);
        assert_eq!(
            state.paths(),
            &["src/lib.rs".to_string(), "src/nested/lib.rs".to_string()]
        );
        assert_eq!(
            state.paths(),
            state
                .graph()
                .files()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>()
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_read_errors_fail_and_pruned_read_errors_have_no_effect() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src/pruned")).unwrap();
        fs::write(root.join("src/selected.rs"), b"pub fn selected() {}\n").unwrap();
        fs::write(root.join("src/pruned/hidden.rs"), b"pub fn hidden() {}\n").unwrap();
        let selected = root.join("src/selected.rs");
        let pruned = root.join("src/pruned");
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o000)).unwrap();
        fs::set_permissions(&pruned, fs::Permissions::from_mode(0o000)).unwrap();
        let config = Config {
            source_roots: vec!["src".into()],
            source_exclude: vec!["src/pruned/**".into()],
            ..Config::default()
        };

        let can_read_without_permission = fs::File::open(&selected).is_ok();
        let result = refresh(root, &config, None);
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&pruned, fs::Permissions::from_mode(0o700)).unwrap();
        if can_read_without_permission {
            return;
        }

        let error = result.unwrap_err().to_string();
        assert!(error.contains("failed to read selected file `src/selected.rs`"));
        assert!(!error.contains("src/pruned"));
    }

    #[test]
    fn docs_reuse_clears_changes_without_source_work() {
        let (temp, config) = fixture();
        let state = refresh(temp.path(), &config, None).unwrap();
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

        let state = refresh(temp.path(), &config, None).unwrap();

        assert!(state.paths().is_empty());
        assert!(state.graph().files().next().is_none());
        assert_eq!(index_work_counts(), IndexWorkCounts::default());
        assert_eq!(graph::work_counts(), graph::WorkCounts::default());
    }
}

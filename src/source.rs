//! Source discovery, indexing, parsing, and graph construction.

mod catalog;
mod graph;
mod paths;

use crate::Result;
use crate::config::Config;
use crate::discovery::{SourceScope, discover_source_candidates};
use crate::repository::RepositoryFiles;

pub use graph::{
    DirectiveKind, EdgeOrigin, Import, Language, ModuleRelationshipRole, Relationship,
    RelationshipKind, RelationshipTarget, SourceFile, SourceGraph, Symbol, SymbolId, SymbolKind,
};
pub use paths::{read_selected, read_selected_text};

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
        crate::work::record(crate::work::Work::DiscoveryScan);
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

    pub(crate) fn resolve_partial_path(&self, query: &str) -> Option<(String, bool)> {
        self.catalog.resolve_partial_path(query)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::work::{self, Work};

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
            Config::parse(Some("[source]\nroots = [\"src\"]\nexclude = []\n")).unwrap(),
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
        work::reset();

        let state = refresh(temp.path(), &config, None).unwrap();

        assert_eq!(work::count(Work::SourceRead), 3);
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
        let config = Config::parse(Some(
            "[source]\nroots = [\"src\"]\nexclude = [\"src/pruned/**\"]\n",
        ))
        .unwrap();

        let can_read_without_permission = fs::File::open(&selected).is_ok();
        let result = refresh(root, &config, None);
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&pruned, fs::Permissions::from_mode(0o700)).unwrap();
        if can_read_without_permission {
            eprintln!(
                "skipped selected_read_errors_fail_and_pruned_read_errors_have_no_effect: \
                 this user reads files without permission bits (root)"
            );
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
        assert!(!state.graph().changed_files().is_empty());
        work::reset();

        let reused = state.reuse_for_docs();

        assert_eq!(reused.paths(), state.paths());
        assert!(reused.graph().changed_files().is_empty());
        assert_eq!(work::total(), 0);
    }

    #[test]
    fn disabled_state_does_no_source_work() {
        let (temp, _) = fixture();
        let config = Config::parse(Some("[index]\nsource = false\n")).unwrap();
        work::reset();

        let state = refresh(temp.path(), &config, None).unwrap();

        assert!(state.paths().is_empty());
        assert!(state.graph().files().next().is_none());
        assert_eq!(work::total(), 0);
    }
}

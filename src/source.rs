//! Source discovery, indexing, parsing, and graph construction.

mod catalog;
mod graph;
mod paths;

pub(crate) use catalog::{IndexedSource, SourceBuild, SourceCatalog};
pub(crate) use graph::{
    Language, SourceFile, SourceGraph, SourceGraphBuild, SymbolKind, load_cached,
};
pub(crate) use paths::read_source_to_string;

#[cfg(test)]
pub(crate) use catalog::{
    WorkCounts as IndexWorkCounts, lock_live_test, reset_work_counts as reset_index_work_counts,
    work_counts as index_work_counts,
};
#[cfg(test)]
pub(crate) use graph::{
    WorkCounts as GraphWorkCounts, reset_work_counts as reset_graph_work_counts,
    work_counts as graph_work_counts,
};

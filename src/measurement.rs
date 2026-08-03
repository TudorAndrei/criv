//! Disabled-by-default command-local performance measurement.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::util::write_atomic_in;

const OUTPUT_PATH_ENV: &str = "CRIV_PERF_MEASUREMENT_PATH";
const SCHEMA: &str = "criv.performance-measurement.v1";

#[derive(Debug, Clone, Copy)]
pub(crate) enum Counter {
    NotesLoaded,
    NoteBytes,
    SourceCatalogTraversals,
    SourceEnumerations,
    SourceFilesIndexed,
    SourceReads,
    SourceBytes,
    SourceGraphCacheLoads,
    SourceGraphParsedFiles,
    SourceGraphReusedFiles,
    SourceGraphCacheSerializations,
    SourceGraphCachePublications,
    SourceGraphPublishedBytes,
    SourceLinkResolutions,
    SourceTargetResolutions,
    PolicyDefinitions,
    PolicyScopeResolutions,
    PolicyCompilations,
    AstParses,
    StateBuilds,
    StateSerializations,
    StatePublications,
    StatePublishedBytes,
    PublishedBytes,
}

impl Counter {
    fn name(self) -> &'static str {
        match self {
            Self::NotesLoaded => "notes_loaded",
            Self::NoteBytes => "note_bytes",
            Self::SourceCatalogTraversals => "source_catalog_traversals",
            Self::SourceEnumerations => "source_enumerations",
            Self::SourceFilesIndexed => "source_files_indexed",
            Self::SourceReads => "source_reads",
            Self::SourceBytes => "source_bytes",
            Self::SourceGraphCacheLoads => "source_graph_cache_loads",
            Self::SourceGraphParsedFiles => "source_graph_parsed_files",
            Self::SourceGraphReusedFiles => "source_graph_reused_files",
            Self::SourceGraphCacheSerializations => "source_graph_cache_serializations",
            Self::SourceGraphCachePublications => "source_graph_cache_publications",
            Self::SourceGraphPublishedBytes => "source_graph_published_bytes",
            Self::SourceLinkResolutions => "source_link_resolutions",
            Self::SourceTargetResolutions => "source_target_resolutions",
            Self::PolicyDefinitions => "policy_definitions",
            Self::PolicyScopeResolutions => "policy_scope_resolutions",
            Self::PolicyCompilations => "policy_compilations",
            Self::AstParses => "ast_parses",
            Self::StateBuilds => "state_builds",
            Self::StateSerializations => "state_serializations",
            Self::StatePublications => "state_publications",
            Self::StatePublishedBytes => "state_published_bytes",
            Self::PublishedBytes => "published_bytes",
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct SpanSummary {
    invocations: u64,
    seconds: f64,
}

#[derive(Debug, Serialize)]
struct MeasurementRecord {
    schema: &'static str,
    run_id: Option<String>,
    sample_id: Option<String>,
    case: Option<String>,
    cache_state: Option<String>,
    success: bool,
    counters: BTreeMap<&'static str, u64>,
    spans: BTreeMap<&'static str, SpanSummary>,
}

struct ActiveMeasurement {
    root: PathBuf,
    output_path: PathBuf,
    started: Instant,
    record: MeasurementRecord,
}

thread_local! {
    static ACTIVE: RefCell<Option<ActiveMeasurement>> = const { RefCell::new(None) };
}

pub(crate) fn begin_command(root: &Path) {
    ACTIVE.with(|active| *active.borrow_mut() = None);
    let Some(output_path) = std::env::var_os(OUTPUT_PATH_ENV).map(PathBuf::from) else {
        return;
    };
    if !confined_measurement_path(&output_path) {
        return;
    }
    let record = MeasurementRecord {
        schema: SCHEMA,
        run_id: environment_text("CRIV_PERF_RUN_ID"),
        sample_id: environment_text("CRIV_PERF_SAMPLE_ID"),
        case: environment_text("CRIV_PERF_CASE"),
        cache_state: environment_text("CRIV_PERF_CACHE_STATE"),
        success: false,
        counters: BTreeMap::new(),
        spans: BTreeMap::new(),
    };
    ACTIVE.with(|active| {
        *active.borrow_mut() = Some(ActiveMeasurement {
            root: root.to_path_buf(),
            output_path,
            started: Instant::now(),
            record,
        });
    });
}

pub(crate) fn finish_command(success: bool) {
    let Some(mut active) = ACTIVE.with(|current| current.borrow_mut().take()) else {
        return;
    };
    active.record.success = success;
    record_span(
        &mut active.record.spans,
        "command.total",
        active.started.elapsed().as_secs_f64(),
    );
    let Ok(mut contents) = serde_json::to_string_pretty(&active.record) else {
        return;
    };
    contents.push('\n');
    let _ = write_atomic_in(
        &active.root,
        Path::new(".criv"),
        &active.output_path,
        &contents,
    );
}

pub(crate) fn add(counter: Counter, amount: usize) {
    if amount == 0 {
        return;
    }
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let Some(active) = active.as_mut() else {
            return;
        };
        *active.record.counters.entry(counter.name()).or_default() += amount as u64;
    });
}

pub(crate) fn increment(counter: Counter) {
    add(counter, 1);
}

pub(crate) fn span(name: &'static str) -> MeasurementSpan {
    MeasurementSpan {
        name,
        started: ACTIVE.with(|active| active.borrow().as_ref().map(|_| Instant::now())),
    }
}

pub(crate) struct MeasurementSpan {
    name: &'static str,
    started: Option<Instant>,
}

impl Drop for MeasurementSpan {
    fn drop(&mut self) {
        let Some(started) = self.started else {
            return;
        };
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let Some(active) = active.as_mut() else {
                return;
            };
            record_span(
                &mut active.record.spans,
                self.name,
                started.elapsed().as_secs_f64(),
            );
        });
    }
}

fn record_span(spans: &mut BTreeMap<&'static str, SpanSummary>, name: &'static str, seconds: f64) {
    let span = spans.entry(name).or_default();
    span.invocations += 1;
    span.seconds += seconds;
}

fn confined_measurement_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.starts_with(".criv")
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        && path.file_name().is_some()
}

fn environment_text(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_paths_are_confined_to_criv_state() {
        assert!(confined_measurement_path(Path::new(
            ".criv/performance-measurement.json"
        )));
        assert!(!confined_measurement_path(Path::new("measurement.json")));
        assert!(!confined_measurement_path(Path::new(
            ".criv/../escape.json"
        )));
        assert!(!confined_measurement_path(Path::new(
            "/tmp/measurement.json"
        )));
    }
}

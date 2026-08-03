use std::path::Path;
use std::sync::Arc;

use crate::Result;
use crate::architecture;
use crate::check;
use crate::source_graph::{self, SourceGraphBuild};
use crate::source_index::SourceIndex;
use crate::state::{self, State};
use crate::vault::Vault;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RefreshCause {
    Initial,
    DocsChanged,
    SourceChanged,
}

#[derive(Debug)]
pub(crate) struct RefreshResult {
    vault: Vault,
    state: State,
}

#[derive(Debug)]
pub(crate) struct RefreshSession {
    seed_graph: Option<SourceGraphBuild>,
    source_index: Option<Arc<dyn SourceIndex>>,
    previous: Option<RefreshResult>,
}

impl RefreshSession {
    pub(crate) fn one_shot(root: &Path) -> Self {
        Self {
            seed_graph: source_graph::load_cached(root),
            source_index: None,
            previous: None,
        }
    }

    pub(crate) fn live(root: &Path, source_index: Option<Arc<dyn SourceIndex>>) -> Self {
        Self {
            seed_graph: source_graph::load_cached(root),
            source_index,
            previous: None,
        }
    }

    pub(crate) fn refresh(&mut self, root: &Path, cause: RefreshCause) -> Result<&RefreshResult> {
        let previous_graph = self
            .previous
            .as_ref()
            .map(|previous| previous.vault.source_graph_build())
            .or(self.seed_graph.as_ref());
        let previous_state = if matches!(cause, RefreshCause::SourceChanged) {
            self.previous.as_ref().map(|previous| &previous.state)
        } else {
            None
        };
        let next = execute(
            root,
            previous_graph,
            previous_state,
            self.source_index.clone(),
        )?;

        self.seed_graph = None;
        self.previous = Some(next);
        Ok(self
            .previous
            .as_ref()
            .expect("refresh result was just stored"))
    }

    pub(crate) fn source_fingerprint(&self) -> Result<Option<String>> {
        self.source_index
            .as_ref()
            .map(|source_index| source_index.source_fingerprint())
            .transpose()
    }
}

fn execute(
    root: &Path,
    previous_graph: Option<&SourceGraphBuild>,
    previous_state: Option<&State>,
    source_index: Option<Arc<dyn SourceIndex>>,
) -> Result<RefreshResult> {
    let mut vault =
        Vault::load_incremental_with_source_index(root, previous_graph, source_index.clone())?;
    let changed_files = vault.source_graph().changed_files().to_vec();
    if architecture::write_code_architecture(root, &vault)? {
        let refreshed_graph = vault.source_graph_build().clone();
        vault =
            Vault::load_incremental_with_source_index(root, Some(&refreshed_graph), source_index)?;
        vault.retain_source_graph_changes_from(&refreshed_graph);
    }

    let diagnostics = check::validate_with_previous_state(&vault, previous_state);
    let (snapshot, state) = match previous_state {
        Some(previous_state) => {
            state::write_state_incremental(root, &vault, Some(previous_state), &changed_files)?
        }
        None => state::write_state(root, &vault)?,
    };
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();
    println!("state updated: snapshot {snapshot}, {errors} errors, {warnings} warnings");

    Ok(RefreshResult { vault, state })
}

#[cfg(test)]
impl RefreshResult {
    fn vault(&self) -> &Vault {
        &self.vault
    }

    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests;

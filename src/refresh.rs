use std::path::Path;

use crate::architecture;
use crate::check;
use crate::config::Config;
use crate::source_graph::{self, SourceGraphBuild};
use crate::source_index::{LiveSourceIndex, OneShotSourceIndex, SourceChange, SourceIndexHandle};
use crate::state::{self, State};
use crate::vault::Vault;
use crate::{CrivError, Result};

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
    source_index: RefreshSourceIndex,
    previous: Option<RefreshResult>,
}

#[derive(Debug)]
enum RefreshSourceIndex {
    OneShot(OneShotSourceIndex),
    Live(LiveSourceIndex),
}

impl RefreshSourceIndex {
    fn handle(&self) -> SourceIndexHandle {
        match self {
            Self::OneShot(index) => index.handle(),
            Self::Live(index) => index.handle(),
        }
    }

    fn observe_source_change(&mut self) -> Result<SourceChange> {
        match self {
            Self::OneShot(_) => Ok(SourceChange::Unchanged),
            Self::Live(index) => index.observe_source_change(),
        }
    }
}

impl RefreshSession {
    pub(crate) fn one_shot(root: &Path) -> Result<Self> {
        let config = Config::load(root)?;
        Ok(Self {
            seed_graph: source_graph::load_cached(root),
            source_index: RefreshSourceIndex::OneShot(OneShotSourceIndex::new(root, &config)?),
            previous: None,
        })
    }

    pub(crate) fn live(root: &Path, config: &Config) -> Result<Self> {
        Ok(Self {
            seed_graph: source_graph::load_cached(root),
            source_index: RefreshSourceIndex::Live(LiveSourceIndex::new(root, config)?),
            previous: None,
        })
    }

    pub(crate) fn refresh(&mut self, root: &Path, cause: RefreshCause) -> Result<&RefreshResult> {
        let previous_graph = self
            .previous
            .as_ref()
            .map(|previous| previous.vault.source_graph_build())
            .or(self.seed_graph.as_ref());
        let previous_state = self.previous.as_ref().map(|previous| &previous.state);
        let diagnostic_previous_state = matches!(cause, RefreshCause::SourceChanged)
            .then_some(previous_state)
            .flatten();
        let next = execute(
            root,
            previous_graph,
            previous_state,
            diagnostic_previous_state,
            self.source_index.handle(),
        )?;

        self.seed_graph = None;
        self.previous = Some(next);
        Ok(self
            .previous
            .as_ref()
            .expect("refresh result was just stored"))
    }

    pub(crate) fn observe_source_change(&mut self) -> Result<SourceChange> {
        self.source_index.observe_source_change()
    }
}

fn execute(
    root: &Path,
    previous_graph: Option<&SourceGraphBuild>,
    previous_state: Option<&State>,
    diagnostic_previous_state: Option<&State>,
    source_index: SourceIndexHandle,
) -> Result<RefreshResult> {
    let mut vault =
        Vault::load_incremental_with_source_index(root, previous_graph, source_index.clone())?;
    let blockers = check::publication_blocking_diagnostics(&vault);
    if !blockers.is_empty() {
        return Err(CrivError::new(format!(
            "state publication blocked by unresolved effective ADR governance:\n{}",
            blockers
                .iter()
                .map(check::Diagnostic::describe)
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    let changed_files = vault.source_graph().changed_files().to_vec();
    if architecture::write_code_architecture(root, &vault)? {
        let refreshed_graph = vault.source_graph_build().clone();
        vault =
            Vault::load_incremental_with_source_index(root, Some(&refreshed_graph), source_index)?;
        vault.retain_source_graph_changes_from(&refreshed_graph);
    }

    let diagnostics = check::validate_with_previous_state(&vault, diagnostic_previous_state);
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

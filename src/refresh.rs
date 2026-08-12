use std::path::Path;

use crate::check;
use crate::config::Config;
use crate::policy_scan::PolicyScanPlan;
use crate::source_graph::{self, SourceGraphBuild};
use crate::source_index::{SourceCatalog, SourceChange, SourceIndexLifecycle, SourceObservation};
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
    config: Config,
    seed_graph: Option<SourceGraphBuild>,
    source_index: SourceIndexLifecycle,
    pending_observation: Option<SourceObservation>,
    previous: Option<RefreshResult>,
}

impl RefreshSession {
    pub(crate) fn one_shot(root: &Path) -> Result<Self> {
        let config = Config::load(root)?;
        Ok(Self {
            config: config.clone(),
            seed_graph: source_graph::load_cached(root),
            source_index: SourceIndexLifecycle::for_command(root, &config)?,
            pending_observation: None,
            previous: None,
        })
    }

    pub(crate) fn live(root: &Path, config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            seed_graph: source_graph::load_cached(root),
            source_index: SourceIndexLifecycle::for_watch(root, config)?,
            pending_observation: None,
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
        let observation = match self.pending_observation.take() {
            Some(observation) => observation,
            None => self.source_index.observe()?,
        };
        let next = execute(
            root,
            &self.config,
            previous_graph,
            previous_state,
            diagnostic_previous_state,
            observation.into_catalog(),
        )?;

        self.seed_graph = None;
        self.previous = Some(next);
        Ok(self
            .previous
            .as_ref()
            .expect("refresh result was just stored"))
    }

    pub(crate) fn observe_source_change(&mut self) -> Result<SourceChange> {
        self.pending_observation = None;
        let observation = self.source_index.observe()?;
        let change = observation.change();
        self.pending_observation = Some(observation);
        Ok(change)
    }
}

fn execute(
    root: &Path,
    config: &Config,
    previous_graph: Option<&SourceGraphBuild>,
    previous_state: Option<&State>,
    diagnostic_previous_state: Option<&State>,
    source_catalog: SourceCatalog,
) -> Result<RefreshResult> {
    let vault = Vault::load_incremental_with_config_and_source_catalog(
        root,
        config,
        previous_graph,
        source_catalog,
    )?;
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

    let policy_plan = PolicyScanPlan::new(&vault);
    let diagnostics = check::validate_with_previous_state_and_policy_plan(
        &vault,
        diagnostic_previous_state,
        &policy_plan,
    );
    let (snapshot, state) = match previous_state {
        Some(previous_state) => state::write_state_incremental_with_policy_plan(
            root,
            &vault,
            Some(previous_state),
            &changed_files,
            &policy_plan,
        )?,
        None => state::write_state_with_policy_plan(root, &vault, &policy_plan)?,
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

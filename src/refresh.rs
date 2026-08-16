use std::path::Path;

use crate::check;
use crate::config::Config;
use crate::policy_scan::PolicyScanPlan;
use crate::source::{self, SourceCatalog, SourceGraphBuild};
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
    source_catalog: Option<SourceCatalog>,
    previous: Option<RefreshResult>,
}

impl RefreshSession {
    pub(crate) fn one_shot(root: &Path) -> Result<Self> {
        let config = Config::load(root)?;
        Ok(Self {
            config: config.clone(),
            seed_graph: source::load_cached(root),
            source_catalog: None,
            previous: None,
        })
    }

    pub(crate) fn live(root: &Path, config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            seed_graph: source::load_cached(root),
            source_catalog: None,
            previous: None,
        })
    }

    pub(crate) fn refresh(&mut self, root: &Path, cause: RefreshCause) -> Result<&RefreshResult> {
        self.refresh_with_precommit_check(root, cause, || Ok(()))
    }

    pub(crate) fn refresh_with_precommit_check(
        &mut self,
        root: &Path,
        cause: RefreshCause,
        precommit_check: impl FnOnce() -> Result<()>,
    ) -> Result<&RefreshResult> {
        let previous_graph = self
            .previous
            .as_ref()
            .map(|previous| previous.vault.source_graph_build())
            .or(self.seed_graph.as_ref());
        let previous_state = self.previous.as_ref().map(|previous| &previous.state);
        let diagnostic_previous_state = matches!(cause, RefreshCause::SourceChanged)
            .then_some(previous_state)
            .flatten();
        let source_catalog = match (cause, self.source_catalog.as_ref()) {
            (RefreshCause::DocsChanged, Some(catalog)) => catalog.clone(),
            _ => SourceCatalog::discover(root, &self.config)?,
        };
        let next = match execute(
            root,
            &self.config,
            previous_graph,
            previous_state,
            diagnostic_previous_state,
            source_catalog.clone(),
            precommit_check,
        ) {
            Ok(next) => next,
            Err(error) => {
                if cause == RefreshCause::SourceChanged {
                    self.source_catalog = None;
                }
                return Err(error);
            }
        };

        self.seed_graph = None;
        self.source_catalog = Some(source_catalog);
        self.previous = Some(next);
        Ok(self
            .previous
            .as_ref()
            .expect("refresh result was just stored"))
    }
}
fn execute(
    root: &Path,
    config: &Config,
    previous_graph: Option<&SourceGraphBuild>,
    previous_state: Option<&State>,
    diagnostic_previous_state: Option<&State>,
    source_catalog: SourceCatalog,
    precommit_check: impl FnOnce() -> Result<()>,
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
        Some(previous_state) => state::write_state_incremental_with_policy_plan_and_check(
            root,
            &vault,
            Some(previous_state),
            &changed_files,
            &policy_plan,
            precommit_check,
        )?,
        None => state::write_state_with_policy_plan_and_check(
            root,
            &vault,
            &policy_plan,
            precommit_check,
        )?,
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

#[cfg(test)]
use std::path::Path;

use crate::check;
use crate::config::Config;
use crate::policy_scan::PolicyScanPlan;
use crate::repository::RepositoryFiles;
use crate::source::SourceState;
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
    files: RepositoryFiles,
    config: Config,
    source_refresh_pending: bool,
    previous: Option<RefreshResult>,
}

impl RefreshSession {
    #[cfg(test)]
    fn one_shot(root: &Path) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::one_shot_from(&files)
    }

    pub(crate) fn one_shot_from(files: &RepositoryFiles) -> Result<Self> {
        let config = Config::load_from(files)?;
        Ok(Self {
            files: files.clone(),
            config,
            source_refresh_pending: false,
            previous: None,
        })
    }

    #[cfg(test)]
    fn live(root: &Path, config: &Config) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::live_from(&files, config)
    }

    pub(crate) fn live_from(files: &RepositoryFiles, config: &Config) -> Result<Self> {
        Ok(Self {
            files: files.clone(),
            config: config.clone(),
            source_refresh_pending: false,
            previous: None,
        })
    }

    pub(crate) fn refresh(&mut self, cause: RefreshCause) -> Result<&RefreshResult> {
        self.refresh_with_precommit_check(cause, || Ok(()))
    }

    pub(crate) fn refresh_with_precommit_check(
        &mut self,
        cause: RefreshCause,
        precommit_check: impl FnOnce() -> Result<()>,
    ) -> Result<&RefreshResult> {
        if cause == RefreshCause::SourceChanged {
            self.source_refresh_pending = true;
        }
        let previous_source = self
            .previous
            .as_ref()
            .map(|previous| previous.vault.source_state());
        let previous_state = self.previous.as_ref().map(|previous| &previous.state);
        let diagnostic_previous_state = matches!(cause, RefreshCause::SourceChanged)
            .then_some(previous_state)
            .flatten();
        let source = match (cause, previous_source, self.source_refresh_pending) {
            (RefreshCause::DocsChanged, Some(source), false) => source.reuse_for_docs(),
            _ => SourceState::refresh_from(&self.files, &self.config, previous_source)?,
        };
        let next = execute(
            &self.files,
            &self.config,
            previous_state,
            diagnostic_previous_state,
            source,
            precommit_check,
        )?;

        self.source_refresh_pending = false;
        self.previous = Some(next);
        Ok(self
            .previous
            .as_ref()
            .expect("refresh result was just stored"))
    }
}
fn execute(
    files: &RepositoryFiles,
    config: &Config,
    previous_state: Option<&State>,
    diagnostic_previous_state: Option<&State>,
    source: SourceState,
    precommit_check: impl FnOnce() -> Result<()>,
) -> Result<RefreshResult> {
    let vault = Vault::load_incremental_with_config_and_source_state(files, config, source)?;
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
    let changed_files = vault.source_state().changed_files().to_vec();

    let policy_plan = PolicyScanPlan::new(&vault);
    let diagnostics = check::validate_with_previous_state_and_policy_plan(
        &vault,
        diagnostic_previous_state,
        &policy_plan,
    );
    let (snapshot, state) = match previous_state {
        Some(previous_state) => state::write_state_incremental_with_policy_plan_and_check(
            &vault,
            Some(previous_state),
            &changed_files,
            &policy_plan,
            precommit_check,
        )?,
        None => {
            state::write_state_with_policy_plan_and_check(&vault, &policy_plan, precommit_check)?
        }
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

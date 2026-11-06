use crate::Result;
use crate::check;
use crate::config::Config;
use crate::repository::RepositoryFiles;
use crate::source::SourceState;
use crate::state::{self, State};
use crate::vault::{LoadOptions, SourceLoad, Vault};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum RefreshCause {
    Initial,
    DocsChanged,
    SourceChanged,
}

/// The last published refresh. Its Vault owns the Source state that the next
/// refresh reuses (ADR-0126).
#[derive(Debug)]
pub(super) struct RefreshResult {
    vault: Vault,
    state: State,
    pub(super) snapshot: String,
    pub(super) errors: usize,
    pub(super) warnings: usize,
}

#[derive(Debug)]
pub(super) struct RefreshSession {
    files: RepositoryFiles,
    config: Config,
    source_refresh_pending: bool,
    previous: Option<RefreshResult>,
}

impl RefreshSession {
    pub(super) fn new(files: &RepositoryFiles, config: &Config) -> Self {
        Self {
            files: files.clone(),
            config: config.clone(),
            source_refresh_pending: false,
            previous: None,
        }
    }

    /// Publishes once `precommit_check` passes. The result replaces the
    /// previous one only after the State commit; a failed Source refresh is
    /// observed again by the next refresh, whatever its cause.
    pub(super) fn refresh(
        &mut self,
        cause: RefreshCause,
        precommit_check: impl FnOnce() -> Result<()>,
    ) -> Result<&RefreshResult> {
        if cause == RefreshCause::SourceChanged {
            self.source_refresh_pending = true;
        }
        let previous = self.previous.as_ref();
        let source = match (cause, previous, self.source_refresh_pending) {
            (RefreshCause::DocsChanged, Some(previous), false) => {
                previous.vault.source_state().reuse_for_docs()
            }
            _ => SourceState::refresh(
                &self.files,
                &self.config,
                previous.map(|previous| previous.vault.source_state()),
            )?,
        };
        let vault = Vault::load(
            &self.files,
            LoadOptions {
                config: Some(self.config.clone()),
                source: SourceLoad::Observed(source),
            },
        )?;
        let validation = check::validate_for_publication(&vault)?;
        let (snapshot, state) = state::write_state(
            &vault,
            previous.map(|previous| &previous.state),
            &validation.policies,
            precommit_check,
        )?;
        let diagnostics = validation.diagnostics;
        let result = RefreshResult {
            vault,
            state,
            snapshot,
            errors: diagnostics.iter().filter(|diag| diag.is_error()).count(),
            warnings: diagnostics.iter().filter(|diag| diag.is_warning()).count(),
        };
        self.source_refresh_pending = false;
        Ok(self.previous.insert(result))
    }
}

impl RefreshResult {
    pub(super) fn text_summary(&self) -> String {
        format!(
            "state updated: snapshot {}, {} errors, {} warnings",
            self.snapshot, self.errors, self.warnings
        )
    }
}

#[cfg(test)]
mod tests;

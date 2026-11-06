//! Architecture decision records: the immutability gate that enforcement
//! runs, and the commands that reconcile a branch with its target.

mod gate;
mod governs;
mod identity_reconcile;
mod receipt;
mod reconcile_transaction;
mod source_reconcile;

use std::path::Path;

use usage_rs::{Args as UsageArgs, Subcommands};

use crate::Result;
use crate::repository::RepositoryFiles;

pub use gate::change_violations;
pub use reconcile_transaction::Mode;

#[derive(Debug, UsageArgs)]
pub struct AdrOptions {
    #[usage(subcommand)]
    command: AdrCommand,
}

#[derive(Debug, Subcommands)]
enum AdrCommand {
    /// Reconcile provisional ADR IDs against an integration target.
    Reconcile(ReconcileOptions),
    /// Reconcile exact governed source renames against an integration target.
    ReconcileSources(ReconcileOptions),
}

#[derive(Debug, UsageArgs)]
struct ReconcileOptions {
    /// Target branch or commit to compare with.
    #[usage(long)]
    base: String,
    /// Report a required reconciliation without modifying the worktree.
    #[usage(long)]
    check: bool,
}

pub fn run(root: &Path, options: &AdrOptions) -> Result<()> {
    let files = crate::config::open_vault(root)?;
    match &options.command {
        AdrCommand::Reconcile(options) => {
            reconcile_identities(&files, &options.base, options.mode())
        }
        AdrCommand::ReconcileSources(options) => reconcile_transaction::run::<
            source_reconcile::SourceReconciliation,
        >(&files, &options.base, options.mode()),
    }
}

/// Renumber branch-local ADRs that collide with `base_ref`. CI runs the
/// [`Mode::Check`] form before enforcement.
pub fn reconcile_identities(files: &RepositoryFiles, base_ref: &str, mode: Mode) -> Result<()> {
    reconcile_transaction::run::<identity_reconcile::IdentityReconciliation>(files, base_ref, mode)
}

impl ReconcileOptions {
    const fn mode(&self) -> Mode {
        if self.check { Mode::Check } else { Mode::Write }
    }
}

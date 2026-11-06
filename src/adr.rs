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
use crate::git::GitRepository;
use crate::repository::RepositoryFiles;

pub use gate::change_violations;
use reconcile_transaction::Mode;

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
        AdrCommand::Reconcile(options) => reconcile_transaction::run::<
            identity_reconcile::IdentityReconciliation,
        >(&files, &options.base, options.mode()),
        AdrCommand::ReconcileSources(options) => reconcile_transaction::run::<
            source_reconcile::SourceReconciliation,
        >(&files, &options.base, options.mode()),
    }
}

/// Why the branch's ADR IDs collide with `base_ref`, or `None` when the
/// allocation is current. CI enforcement asks this before it scans.
pub fn identity_conflict(
    files: &RepositoryFiles,
    git: &GitRepository,
    base_ref: &str,
) -> Result<Option<String>> {
    reconcile_transaction::conflict::<identity_reconcile::IdentityReconciliation>(
        files, git, base_ref,
    )
}

impl ReconcileOptions {
    const fn mode(&self) -> Mode {
        if self.check { Mode::Check } else { Mode::Write }
    }
}

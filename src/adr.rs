//! Architecture decision records: the command that creates a new decision,
//! the gate that enforcement runs, and the command that reconciles governed
//! source renames with a target.

mod create;
mod gate;
mod governs;
mod receipt;
mod reconcile_transaction;
mod source_reconcile;

use std::path::Path;

use usage_rs::{Args as UsageArgs, Subcommands};

use crate::Result;

pub use gate::{change_violations, legacy_id_violations};
use reconcile_transaction::Mode;

/// Create and maintain decision records.
#[derive(Debug, UsageArgs)]
pub struct AdrOptions {
    #[usage(subcommand)]
    command: AdrCommand,
}

#[derive(Debug, Subcommands)]
enum AdrCommand {
    /// Create the draft of a new decision and print its path.
    ///
    /// The draft is `<adr dir>/YYYYMMDD-<slug>.md` with the id
    /// `ADR-YYYYMMDD-<slug>`, the title, `status: draft`, and empty Context,
    /// Decision, and Consequences sections. The command fails, and writes
    /// nothing, when the vault already holds that id.
    New(NewOptions),
    /// Reconcile exact governed source renames against an integration target.
    ReconcileSources(ReconcileOptions),
}

#[derive(Debug, UsageArgs)]
struct NewOptions {
    /// Decision title; its slug ends the id and the file name.
    title: String,
    /// Decision date as YYYYMMDD; defaults to today's UTC date.
    #[usage(long)]
    date: Option<String>,
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
        AdrCommand::New(options) => {
            let path = create::create(&files, &options.title, options.date.as_deref())?;
            println!("{path}");
            Ok(())
        }
        AdrCommand::ReconcileSources(options) => reconcile_transaction::run::<
            source_reconcile::SourceReconciliation,
        >(&files, &options.base, options.mode()),
    }
}

impl ReconcileOptions {
    const fn mode(&self) -> Mode {
        if self.check { Mode::Check } else { Mode::Write }
    }
}

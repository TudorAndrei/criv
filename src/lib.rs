mod check;
mod config;
mod enforce;
mod init;
mod query;
mod search;
mod source_graph;
mod state;
mod util;
mod vault;
mod watch;

use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};

pub type Result<T> = std::result::Result<T, CrivError>;

#[derive(Debug, thiserror::Error)]
pub enum CrivError {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl CrivError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Message(_) | Self::Io(_) => 1,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "criv",
    version,
    about = "Local docs-to-code knowledge graph validator and query tool",
    after_help = "Implemented query names: next-adr-id, targets, references, cites, cited-by, governs, governing, coverage, nodes, callers, callees, attack-surface, diff, orphan-docs."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(init::InitOptions),
    Check(check::CheckOptions),
    Query(query::QueryOptions),
    Search(search::SearchOptions),
    Watch(watch::WatchOptions),
    Enforce(enforce::EnforceOptions),
}

pub fn run(args: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let cli = match Cli::try_parse_from(std::iter::once("criv".to_string()).chain(args)) {
        Ok(cli) => cli,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            print!("{err}");
            return Ok(());
        }
        Err(err) => return Err(CrivError::usage(err.to_string())),
    };

    match cli.command {
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
        Some(Command::Init(options)) => init::run(&cwd, options),
        Some(Command::Check(options)) => check::run(&cwd, options),
        Some(Command::Query(options)) => query::run(&cwd, options),
        Some(Command::Search(options)) => search::run(&cwd, options),
        Some(Command::Watch(options)) => watch::run(&cwd, options),
        Some(Command::Enforce(options)) => enforce::run(&cwd, options),
    }
}

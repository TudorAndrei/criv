//! PROTOTYPE. Throwaway. Answers wayfinder ticket #189. Not migration code.
//!
//! It declares the criv root command and the two hard subcommands in
//! `usage-rs` 6: `check` with `conflicts_with` and `default_value_t`, and
//! `enforce` with two hidden `requires` flags.

use std::ffi::{OsStr, OsString};

use usage::{Args, Cli, Error, Subcommands, ValueEnum, help};

/// Local docs-to-code knowledge graph validator and query tool
#[derive(Debug, Cli)]
#[usage(bin = "criv", version = "0.10.1", unknown_flags = "error")]
struct CrivCli {
    #[usage(long = "usage", hide)]
    usage: bool,
    #[usage(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommands)]
enum Command {
    /// Validate the vault against the source graph.
    Check(CheckOptions),
    /// Enforce the policy for one stage.
    Enforce(EnforceOptions),
    /// Query the knowledge graph.
    Query(QueryOptions),
}

#[derive(Debug, Args)]
struct QueryOptions {
    #[usage(subcommand)]
    command: QueryCommand,
}

#[derive(Debug, Subcommands)]
enum QueryCommand {
    /// List source symbols that call the requested symbol.
    Callers(SymbolOptions),
    /// List documentation notes without citations.
    OrphanDocs(OutputOptions),
}

#[derive(Debug, Args)]
struct SymbolOptions {
    /// The symbol to look up.
    symbol: String,
    #[usage(long, value_enum, default = "text")]
    format: Format,
}

#[derive(Debug, Args)]
struct OutputOptions {
    #[usage(long, value_enum, default = "text")]
    format: Format,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Github,
}

#[derive(Debug, Args)]
struct CheckOptions {
    #[usage(long, value_enum, default = "text")]
    format: Format,
    #[usage(long)]
    filter: Option<String>,
    #[usage(long)]
    fix: bool,
    /// Validate safely scoped facts for the staged Git transaction.
    #[usage(long, conflicts_with = "fix")]
    changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Stage {
    Commit,
    Push,
    Ci,
}

#[derive(Debug, Args)]
struct EnforceOptions {
    #[usage(long, value_enum)]
    stage: Stage,
    /// Consume Git's pre-push ref-update records from standard input.
    #[usage(long, hide)]
    pre_push: bool,
    #[usage(long, hide, requires = "pre_push")]
    remote_name: Option<String>,
    #[usage(long, hide, requires = "pre_push")]
    remote_url: Option<String>,
}

fn main() {
    let owned: Vec<OsString> = std::env::args_os().skip(1).collect();
    let argv: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();
    match CrivCli::parse_from(&argv) {
        Ok(cli) => {
            if cli.usage {
                print!("{}", CrivCli::to_kdl());
                return;
            }
            match cli.command {
                // criv prints the root help when no subcommand is given.
                None => print!(
                    "{}",
                    help::render(CrivCli::spec(), CrivCli::command(), false).unwrap()
                ),
                Some(command) => println!("PARSED: {command:?}"),
            }
        }
        Err(Error::Help { cmd, long }) => {
            print!("{}", help::render(CrivCli::spec(), cmd, long).unwrap());
        }
        Err(Error::Version { .. }) => println!("criv 0.10.1"),
        Err(err) => {
            eprint!("{}", usage::render_failure(CrivCli::spec(), &argv, &err));
            std::process::exit(2);
        }
    }
}

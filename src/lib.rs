mod check;
mod config;
mod enforce;
mod init;
mod query;
mod search;
mod state;
mod util;
mod vault;
mod watch;

use std::fmt::{self, Display};

pub type Result<T> = std::result::Result<T, CrivError>;

#[derive(Debug)]
pub struct CrivError {
    message: String,
    exit_code: i32,
}

impl CrivError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl Display for CrivError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for CrivError {}

impl From<std::io::Error> for CrivError {
    fn from(value: std::io::Error) -> Self {
        CrivError::new(value.to_string())
    }
}

pub fn run(args: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut args = Args::new(args);

    match args.next().as_deref() {
        None | Some("-h") | Some("--help") => {
            print_help();
            Ok(())
        }
        Some("init") => init::run(&cwd, init::InitOptions::parse(args)?),
        Some("check") => check::run(&cwd, check::CheckOptions::parse(args)?),
        Some("query") => query::run(&cwd, query::QueryOptions::parse(args)?),
        Some("search") => search::run(&cwd, search::SearchOptions::parse(args)?),
        Some("watch") => watch::run(&cwd, watch::WatchOptions::parse(args)?),
        Some("enforce") => enforce::run(&cwd, enforce::EnforceOptions::parse(args)?),
        Some(cmd) => Err(CrivError::usage(format!(
            "unknown command `{cmd}`\n\nRun `criv --help` for usage."
        ))),
    }
}

#[derive(Debug)]
pub(crate) struct Args {
    items: Vec<String>,
    index: usize,
}

impl Args {
    pub(crate) fn new(items: Vec<String>) -> Self {
        Self { items, index: 0 }
    }

    pub(crate) fn next(&mut self) -> Option<String> {
        let item = self.items.get(self.index).cloned();
        if item.is_some() {
            self.index += 1;
        }
        item
    }

    pub(crate) fn expect_value(&mut self, flag: &str) -> Result<String> {
        self.next()
            .ok_or_else(|| CrivError::usage(format!("missing value for `{flag}`")))
    }
}

fn print_help() {
    println!(
        "criv 0.1.0\n\n\
         Usage:\n  \
           criv init [--no-obsidian] [--no-skills]\n  \
           criv check [--format text|json] [--filter <pattern>]\n  \
           criv query <name> [args...] [--format text|json]\n  \
           criv search (--grep <text>|--files <query>|--notes <text>) [--format text|json]\n  \
           criv watch\n  \
           criv enforce --stage commit|push|ci\n\n\
         Implemented query names: next-adr-id, targets, references, cites, cited-by, governs, governing, coverage, nodes, orphan-docs.\n\
         This build has the CLI/vault/state foundations; tree-sitter, ast-grep, fff, and embeddings are the remaining backend integrations."
    );
}

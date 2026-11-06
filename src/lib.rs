#![cfg_attr(
    test,
    allow(
        clippy::pedantic,
        clippy::nursery,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod adr;
mod check;
mod config;
mod diagnostic;
mod discovery;
mod enforce;
mod git;
mod glob;
mod identity;
mod init;
mod install;
mod markdown;
mod policy_scan;
mod query;
mod repository;
#[cfg(feature = "selection")]
pub mod selection;
mod source;
mod state;
mod structural;
#[cfg(test)]
mod test_support;
mod usage_json;
mod vault;
mod watch;
#[cfg(test)]
mod work;

use std::ffi::{OsStr, OsString};
use std::io::Write;

use usage_rs::{Cli, Error, Subcommands, help};

pub type Result<T> = std::result::Result<T, CrivError>;

#[derive(Debug, thiserror::Error)]
pub enum CrivError {
    #[error("{0}")]
    Message(String),
    #[error(
        "[{code}] {message}{}",
        code.fix().map(|fix| format!("\nfix: {fix}")).unwrap_or_default()
    )]
    Coded {
        code: diagnostic::DiagnosticCode,
        message: String,
    },
    #[error("{0}")]
    Usage(String),
    #[error("")]
    UsageReported,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl CrivError {
    fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    fn coded(code: diagnostic::DiagnosticCode, message: impl Into<String>) -> Self {
        Self::Coded {
            code,
            message: message.into(),
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) | Self::UsageReported => 2,
            Self::Message(_) | Self::Coded { .. } | Self::Io(_) => 1,
        }
    }

    #[must_use]
    pub const fn is_reported(&self) -> bool {
        matches!(self, Self::UsageReported)
    }
}

/// Local docs-to-code knowledge graph validator and query tool
#[derive(Debug, Cli)]
#[usage(bin = "criv", version, unknown_flags = "error")]
struct CrivCli {
    #[usage(long = "usage", hide)]
    usage: bool,
    #[usage(long = "usage-json", hide)]
    usage_json: bool,
    #[usage(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommands)]
enum Command {
    Init(init::InitOptions),
    /// Install the optional local viewer into a selected editor.
    InstallEditor(install::InstallEditorOptions),
    Adr(adr::AdrOptions),
    Check(check::CheckOptions),
    Query(query::QueryOptions),
    Watch(watch::WatchOptions),
    Enforce(enforce::EnforceOptions),
}

/// Runs the command line interface with `args`.
///
/// # Errors
///
/// Returns an error when parsing, output, or the selected command fails.
pub fn run(args: &[String]) -> Result<()> {
    let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
    let os_args: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();

    let cli = match CrivCli::parse_from(&os_args) {
        Ok(cli) => cli,
        Err(Error::Help { cmd, long }) => {
            print!("{}", render_help(cmd, long)?);
            return Ok(());
        }
        Err(Error::Version { .. }) => {
            println!("criv {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Err(err @ (Error::MissingSubcommand | Error::MissingArgsHelp { .. })) => {
            eprint!(
                "{}",
                usage_rs::render_failure(CrivCli::spec(), &os_args, &err)
            );
            return Err(CrivError::UsageReported);
        }
        Err(err) => {
            return Err(CrivError::usage(usage_rs::render_failure(
                CrivCli::spec(),
                &os_args,
                &err,
            )));
        }
    };

    if cli.usage {
        return write_usage_spec(&mut std::io::stdout().lock());
    }

    if cli.usage_json {
        return usage_json::write(CrivCli::spec().root, &mut std::io::stdout().lock());
    }

    let cwd = std::env::current_dir()?;
    let cwd = cwd.as_path();
    match cli.command {
        None => {
            print!("{}", render_help(CrivCli::command(), false)?);
            Ok(())
        }
        Some(Command::Init(options)) => init::run(cwd, &options),
        Some(Command::InstallEditor(options)) => install::install_editor(&options),
        Some(Command::Adr(options)) => adr::run(cwd, &options),
        Some(Command::Check(options)) => check::run(cwd, &options),
        Some(Command::Query(options)) => query::run(cwd, options),
        Some(Command::Watch(options)) => watch::run(cwd, &options),
        Some(Command::Enforce(options)) => enforce::run(cwd, &options),
    }
}

fn render_help(command: &usage_rs::Command<'_>, long: bool) -> Result<String> {
    help::render(CrivCli::spec(), command, long)
        .ok_or_else(|| CrivError::new("failed to render help"))
}

fn write_usage_spec(writer: &mut dyn Write) -> Result<()> {
    write!(writer, "{}", CrivCli::to_kdl())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::{CrivCli, Error, render_help, usage_json, write_usage_spec};

    fn help_for(args: &[&str]) -> String {
        let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
        let argv: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();
        match CrivCli::parse_from(&argv) {
            Err(Error::Help { cmd, long }) => render_help(cmd, long).unwrap(),
            other => panic!("expected a help request, got {other:?}"),
        }
    }

    /// The query subcommands as the query module declares them.
    fn query_subcommands() -> Vec<&'static str> {
        let query = CrivCli::spec()
            .root
            .subcommands()
            .iter()
            .find(|command| command.cmd().name == "query")
            .expect("query command is declared");
        let names: Vec<_> = query
            .subcommands()
            .iter()
            .map(|command| command.cmd().name)
            .collect();
        assert!(names.contains(&"diff"), "query declares {names:?}");
        names
    }

    #[test]
    fn usage_spec_includes_criv_commands() {
        let mut output = Vec::new();
        write_usage_spec(&mut output).unwrap();

        let spec = String::from_utf8(output).expect("usage spec should be utf-8");

        assert!(spec.contains("bin criv"));
        assert!(spec.contains("flag --usage hide=#true"));
        assert!(spec.contains("cmd check"));
        assert!(spec.contains("cmd install-editor"));
        assert!(spec.contains("cmd query"));
        assert!(spec.contains("cmd enforce"));
        assert!(spec.contains("cmd reconcile-sources"));

        let spec: usage_parser::Spec = CrivCli::to_kdl()
            .parse()
            .expect("emitted spec should parse with the usage consumer");
        let query = spec
            .cmd
            .find_subcommand("query")
            .expect("query command should be exported");
        let names = query
            .subcommands
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(names, query_subcommands());
        assert!(query.args.is_empty());
        assert!(query.flags.iter().all(|flag| flag.builtin));
    }

    #[test]
    fn usage_json_exports_visible_command_metadata() {
        let mut output = Vec::new();
        usage_json::write(CrivCli::spec().root, &mut output).unwrap();

        let tree: serde_json::Value =
            serde_json::from_slice(&output).expect("usage JSON should be valid JSON");
        assert_eq!(tree["name"], "criv");
        assert_eq!(tree["path"], "criv");
        assert!(tree["flags"].as_array().is_none_or(Vec::is_empty));

        let query = tree["subcommands"]
            .as_array()
            .expect("root command has subcommands")
            .iter()
            .find(|command| command["name"] == "query")
            .expect("query command is exported");
        assert_eq!(query["path"], "criv query");
        assert!(
            query["subcommands"]
                .as_array()
                .expect("query has subcommands")
                .iter()
                .any(|command| command["path"] == "criv query nodes")
        );

        let check = tree["subcommands"]
            .as_array()
            .expect("root command has subcommands")
            .iter()
            .find(|command| command["name"] == "check")
            .expect("check command is exported");
        let format = check["flags"]
            .as_array()
            .expect("check command has flags")
            .iter()
            .find(|flag| flag["name"] == "format")
            .expect("format flag is exported");
        assert_eq!(format["default"], serde_json::json!(["text"]));
        assert!(
            format["choices"]
                .as_array()
                .expect("format flag has choices")
                .iter()
                .any(|choice| choice == "json")
        );
    }

    #[test]
    fn help_renders_root_and_subcommand_help() {
        let root = help_for(&["help"]);
        let query = help_for(&["help", "query"]);
        let coverage = help_for(&["help", "query", "coverage"]);
        let nodes = help_for(&["help", "query", "nodes"]);

        assert!(root.contains("Usage: criv"));
        assert!(root.contains("Commands:"));
        assert!(!root.contains("--usage"));
        assert!(query.contains("Usage: criv query"));
        assert!(query.contains("<SUBCOMMAND>"));
        let (_, commands) = query.split_once("Commands:").expect("query lists commands");
        for name in query_subcommands() {
            assert!(
                commands
                    .lines()
                    .any(|line| line.split_whitespace().next() == Some(name))
            );
        }
        assert!(coverage.contains("--by <BY>"));
        assert!(!coverage.contains("--kind"));
        assert!(nodes.contains("--kind <KIND>"));
        assert!(nodes.contains("--without-docs"));
        assert!(!nodes.contains("--by"));
    }

    #[test]
    fn help_uses_the_renderer_language() {
        let check = help_for(&["check", "--help"]);
        let enforce = help_for(&["help", "enforce"]);

        assert!(check.contains("Usage: criv check [FLAGS]"));
        assert!(check.contains("Flags:"));
        assert!(enforce.contains("Usage: criv enforce <--stage <STAGE>>"));
    }
}

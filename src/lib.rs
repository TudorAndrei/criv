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
mod c4;
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
mod refresh;
mod repository;
mod source;
mod state;
mod structural;
mod vault;
mod watch;

#[cfg(test)]
#[path = "../scripts/performance/discovery_probe.rs"]
mod discovery_probe;

#[cfg(test)]
fn discovery_probe_source_files(root: &std::path::Path) -> Result<Vec<String>> {
    let config = config::Config::load(root)?;
    Ok(source::SourceState::refresh(root, &config, None)?
        .paths()
        .to_vec())
}

#[cfg(test)]
fn discovery_probe_source_candidates(root: &std::path::Path) -> Result<Vec<String>> {
    let config = config::Config::load(root)?;
    discovery::discover_source_candidates(root, &config)
}

#[cfg(test)]
fn discovery_probe_vault_files(root: &std::path::Path) -> Result<(Vec<String>, Vec<String>)> {
    let config = config::Config::load(root)?;
    let selected = discovery::discover_vault(root, &config.docs_dir)?;
    Ok((selected.markdown, selected.c4))
}

use std::ffi::{OsStr, OsString};
use std::io::Write;

use usage::{Cli, Error, Subcommands, help};

pub type Result<T> = std::result::Result<T, CrivError>;

#[derive(Debug, thiserror::Error)]
pub enum CrivError {
    #[error("{0}")]
    Message(String),
    #[error("[{code}] {message}{}", fix_suffix(.fix.as_deref()))]
    Coded {
        code: &'static str,
        message: String,
        fix: Option<String>,
    },
    #[error("{0}")]
    Usage(String),
    #[error("")]
    UsageReported,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn fix_suffix(fix: Option<&str>) -> String {
    fix.map_or_else(String::new, |fix| format!("\nfix: {fix}"))
}

impl CrivError {
    fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    fn coded_fix(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::Coded {
            code,
            message: message.into(),
            fix: Some(fix.into()),
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
    let cwd = std::env::current_dir()?;
    run_command(args, &cwd)
}

fn run_command(args: &[String], cwd: &std::path::Path) -> Result<()> {
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
            eprint!("{}", usage::render_failure(CrivCli::spec(), &os_args, &err));
            return Err(CrivError::UsageReported);
        }
        Err(err) => {
            return Err(CrivError::usage(usage::render_failure(
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
        return write_usage_json(&mut std::io::stdout().lock());
    }

    match cli.command {
        None => {
            print!("{}", render_help(CrivCli::command(), false)?);
            Ok(())
        }
        Some(Command::Init(options)) => init::run(cwd, &options),
        Some(Command::InstallEditor(options)) => install::install_editor(&options),
        Some(Command::Adr(options)) => adr::run(cwd, options),
        Some(Command::Check(options)) => check::run(cwd, &options),
        Some(Command::Query(options)) => query::run(cwd, options),
        Some(Command::Watch(options)) => watch::run(cwd, &options),
        Some(Command::Enforce(options)) => enforce::run(cwd, options),
    }
}

fn render_help(command: &usage::Command<'_>, long: bool) -> Result<String> {
    help::render(CrivCli::spec(), command, long)
        .ok_or_else(|| CrivError::new("failed to render help"))
}

fn write_usage_spec(writer: &mut dyn Write) -> Result<()> {
    write!(writer, "{}", CrivCli::to_kdl())?;
    Ok(())
}

#[derive(serde::Serialize)]
struct JsonCommand<'a> {
    name: &'a str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    about: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<JsonArg<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    flags: Vec<JsonFlag<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subcommands: Vec<Self>,
}

#[derive(serde::Serialize)]
struct JsonFlag<'a> {
    name: &'a str,
    long: &'a [&'a str],
    required: bool,
    takes_value: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<&'a str>,
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    choices: &'a [&'a str],
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    default: &'a [&'a str],
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    conflicts: &'a [&'a str],
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    requires: &'a [&'a str],
}

#[derive(serde::Serialize)]
struct JsonArg<'a> {
    name: &'a str,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<&'a str>,
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    choices: &'a [&'a str],
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    default: &'a [&'a str],
}

fn json_command<'a>(meta: &'a usage::spec::CommandMeta<'a>, parent: &str) -> JsonCommand<'a> {
    let path = if parent.is_empty() {
        meta.cmd.name.to_string()
    } else {
        format!("{parent} {}", meta.cmd.name)
    };
    JsonCommand {
        name: meta.cmd.name,
        about: meta.about,
        args: meta
            .args
            .iter()
            .filter(|arg| !arg.hide)
            .map(|arg| JsonArg {
                name: arg.arg.name,
                required: arg.required,
                help: arg.help,
                choices: arg.choices,
                default: arg.default,
            })
            .collect(),
        flags: meta
            .flags
            .iter()
            .filter(|flag| !flag.hide)
            .map(|flag| JsonFlag {
                name: flag.flag.name,
                long: flag.flag.longs,
                required: flag.required,
                takes_value: flag.flag.takes_value,
                help: flag.help,
                choices: flag.choices,
                default: flag.default,
                conflicts: flag.conflicts,
                requires: flag.requires,
            })
            .collect(),
        subcommands: meta
            .subcommands
            .iter()
            .filter(|child| !child.hide)
            .map(|child| json_command(child, &path))
            .collect(),
        path,
    }
}

fn write_usage_json(writer: &mut dyn Write) -> Result<()> {
    let spec = CrivCli::spec();
    let tree = json_command(spec.root, "");
    let json = serde_json::to_string_pretty(&tree)
        .map_err(|error| CrivError::new(format!("failed to serialize command tree: {error}")))?;
    writeln!(writer, "{json}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::{CrivCli, Error, render_help, write_usage_json, write_usage_spec};

    fn help_for(args: &[&str]) -> String {
        let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
        let argv: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();
        match CrivCli::parse_from(&argv) {
            Ok(_) => render_help(CrivCli::command(), false).unwrap(),
            Err(Error::Help { cmd, long }) => render_help(cmd, long).unwrap(),
            Err(error) => panic!("expected a help request, got {error:?}"),
        }
    }

    const QUERY_SUBCOMMANDS: [&str; 14] = [
        "next-adr-id",
        "callers",
        "callees",
        "attack-surface",
        "targets",
        "cites",
        "cited-by",
        "orphan-docs",
        "references",
        "governs",
        "governing",
        "coverage",
        "nodes",
        "diff",
    ];

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
        assert!(!spec.contains("cmd state"));
        assert!(!spec.contains("cmd list"));
        assert!(!spec.contains("cmd prune"));
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
        assert_eq!(names, QUERY_SUBCOMMANDS);
        assert!(query.args.is_empty());
        assert!(query.flags.is_empty());
    }

    #[test]
    fn usage_json_exports_visible_command_metadata() {
        let mut output = Vec::new();
        write_usage_json(&mut output).unwrap();

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
        for name in QUERY_SUBCOMMANDS {
            assert!(query.contains(&format!("query {name}")));
        }
        assert!(coverage.contains("--by <BY>"));
        assert!(!coverage.contains("--kind"));
        assert!(nodes.contains("--kind <KIND>"));
        assert!(nodes.contains("--without-docs"));
        assert!(!nodes.contains("--by"));
    }

    #[test]
    fn help_renders_flag_help_forms() {
        let root = help_for(&["--help"]);
        let query = help_for(&["query", "--help"]);

        assert!(root.contains("Usage: criv"));
        assert!(query.contains("Usage: criv query"));
    }

    #[test]
    fn bare_criv_renders_the_short_root_page() {
        let bare = help_for(&[]);

        assert!(bare.contains("Usage: criv"));
        assert!(bare.contains("Commands:"));
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

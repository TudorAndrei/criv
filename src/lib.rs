mod adr;
mod c4;
mod check;
mod config;
mod discovery;
mod enforce;
mod generated_skills;
mod git;
mod init;
mod install_editor;
mod likec4;
mod policy_scan;
mod query;
mod refresh;
mod source;
mod state;
mod structural;
mod util;
mod vault;
mod watch;

#[cfg(test)]
#[path = "../scripts/performance/discovery_probe.rs"]
mod discovery_probe;

#[cfg(test)]
fn discovery_probe_source_files(root: &std::path::Path) -> Result<Vec<String>> {
    let config = config::Config::load(root)?;
    Ok(source::SourceCatalog::discover(root, &config)?
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

use std::io::Write;

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
    fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    fn usage(message: impl Into<String>) -> Self {
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
    about = "Local docs-to-code knowledge graph validator and query tool"
)]
struct Cli {
    #[arg(long = "usage", hide = true)]
    usage: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(init::InitOptions),
    /// Install the optional local viewer into a selected editor.
    InstallEditor(install_editor::InstallEditorOptions),
    Adr(adr::AdrOptions),
    Check(check::CheckOptions),
    Query(query::QueryOptions),
    Watch(watch::WatchOptions),
    Enforce(enforce::EnforceOptions),
}

pub fn run(args: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    run_command(args, &cwd)
}

fn run_command(args: Vec<String>, cwd: &std::path::Path) -> Result<()> {
    if let Some(help) = usage_help(&args) {
        print!("{help}");
        return Ok(());
    }

    let cli =
        match Cli::try_parse_from(std::iter::once("criv").chain(args.iter().map(String::as_str))) {
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
            Err(err) => return Err(CrivError::usage(parse_error(&args, err))),
        };

    if cli.usage {
        write_usage_spec(&mut std::io::stdout().lock());
        return Ok(());
    }

    match cli.command {
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
        Some(Command::Init(options)) => init::run(cwd, options),
        Some(Command::InstallEditor(options)) => install_editor::run(options),
        Some(Command::Adr(options)) => adr::run(cwd, options),
        Some(Command::Check(options)) => check::run(cwd, options),
        Some(Command::Query(options)) => query::run(cwd, options),
        Some(Command::Watch(options)) => watch::run(cwd, options),
        Some(Command::Enforce(options)) => enforce::run(cwd, options),
    }
}

fn parse_error(args: &[String], error: clap::Error) -> String {
    let mut message = error.to_string();
    if error.kind() != ErrorKind::InvalidSubcommand
        || args.first().is_none_or(|argument| argument != "query")
    {
        return message;
    }

    let command = Cli::command();
    let Some(query) = command.find_subcommand("query") else {
        return message;
    };
    let names = query
        .get_subcommands()
        .map(clap::Command::get_name)
        .filter(|name| *name != "help")
        .collect::<Vec<_>>()
        .join(", ");
    message.push_str(&format!("\nValid query subcommands: {names}\n"));
    message
}

fn write_usage_spec(writer: &mut dyn Write) {
    writeln!(writer, "{}", usage_spec()).expect("write usage spec");
}

fn usage_spec() -> usage::Spec {
    let spec: usage::Spec = (&Cli::command()).into();
    spec.to_string()
        .parse()
        .expect("derived usage spec should parse")
}

fn usage_help(args: &[String]) -> Option<String> {
    let (path, long) = help_request(args)?;
    let mut spec = usage_spec();
    remove_hidden_from_help(&mut spec.cmd);
    normalize_help_usage(&mut spec.cmd);
    let command = command_for_path(&spec, &path)?;

    Some(normalize_help_output(&usage::docs::cli::render_help(
        &spec, command, long,
    )))
}

fn remove_hidden_from_help(command: &mut usage::SpecCommand) {
    command.flags.retain(|flag| !flag.hide);
    command.subcommands.retain(|_, command| !command.hide);
    for subcommand in command.subcommands.values_mut() {
        remove_hidden_from_help(subcommand);
    }
}

fn normalize_help_usage(command: &mut usage::SpecCommand) {
    command.usage = clean_required_flag_usage(&command.usage);
    for subcommand in command.subcommands.values_mut() {
        normalize_help_usage(subcommand);
    }
}

fn clean_required_flag_usage(usage: &str) -> String {
    let mut cleaned = String::new();
    let mut rest = usage;

    while let Some(start) = rest.find("<--") {
        cleaned.push_str(&rest[..start]);
        let flag = &rest[start + 1..];
        let Some(end) = flag.find(">>") else {
            cleaned.push_str(&rest[start..]);
            return cleaned;
        };

        cleaned.push_str(&flag[..=end]);
        rest = &flag[end + 2..];
    }

    cleaned.push_str(rest);
    cleaned
}

fn normalize_help_output(help: &str) -> String {
    help.replace("[FLAGS]", "[OPTIONS]")
        .replace("<FLAGS>", "<OPTIONS>")
        .replace("\nFlags:", "\nOptions:")
}

fn help_request(args: &[String]) -> Option<(Vec<&str>, bool)> {
    match args {
        [] => Some((Vec::new(), true)),
        [flag] if flag == "-h" => Some((Vec::new(), false)),
        [flag] if flag == "--help" => Some((Vec::new(), true)),
        [command, flag] if flag == "-h" => Some((vec![command.as_str()], false)),
        [command, flag] if flag == "--help" => Some((vec![command.as_str()], true)),
        [help] if help == "help" => Some((Vec::new(), true)),
        [help, path @ ..] if help == "help" => {
            Some((path.iter().map(String::as_str).collect(), true))
        }
        _ => None,
    }
}

fn command_for_path<'a>(spec: &'a usage::Spec, path: &[&str]) -> Option<&'a usage::SpecCommand> {
    let mut command = &spec.cmd;
    for segment in path {
        command = command.find_subcommand(segment)?;
    }
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::{usage_help, usage_spec, write_usage_spec};

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
        write_usage_spec(&mut output);

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

        let spec = usage_spec();
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
    fn usage_help_renders_root_and_subcommand_help() {
        let root = usage_help(&["help".to_string()]).expect("root help should render");
        let default = usage_help(&[]).expect("default help should render");
        let query = usage_help(&["help".to_string(), "query".to_string()])
            .expect("query help should render");
        let coverage = usage_help(&[
            "help".to_string(),
            "query".to_string(),
            "coverage".to_string(),
        ])
        .expect("coverage help should render");
        let nodes = usage_help(&["help".to_string(), "query".to_string(), "nodes".to_string()])
            .expect("nodes help should render");

        assert!(root.contains("Usage: criv"));
        assert_eq!(root, default);
        assert!(root.contains("Commands:"));
        assert!(!root.contains("--usage"));
        assert!(query.contains("Usage: criv query"));
        assert!(query.contains("<SUBCOMMAND>"));
        for name in QUERY_SUBCOMMANDS {
            assert!(query.contains(&format!("query {name}")));
        }
        assert!(coverage.contains("--by <BY>"));
        assert!(!coverage.contains("--kind"));
        assert!(!coverage.contains("--without-docs"));
        assert!(nodes.contains("--kind <KIND>"));
        assert!(nodes.contains("--without-docs"));
        assert!(!nodes.contains("--by"));
        assert!(root.contains("enforce --stage <STAGE>"));
        assert!(!root.contains("enforce <--stage <STAGE>>"));
    }

    #[test]
    fn usage_help_renders_flag_help_forms() {
        let root = usage_help(&["--help".to_string()]).expect("root help should render");
        let query = usage_help(&["query".to_string(), "--help".to_string()])
            .expect("query help should render");

        assert!(root.contains("Usage: criv"));
        assert!(query.contains("Usage: criv query"));
    }

    #[test]
    fn usage_help_uses_options_language() {
        let check =
            usage_help(&["check".to_string(), "--help".to_string()]).expect("help should render");

        assert!(check.contains("Usage: criv check [OPTIONS]"));
        assert!(check.contains("Options:"));
        assert!(!check.contains("[FLAGS]"));
        assert!(!check.contains("Flags:"));
    }

    #[test]
    fn usage_help_cleans_required_flag_placeholders() {
        let enforce =
            usage_help(&["help".to_string(), "enforce".to_string()]).expect("help should render");

        assert!(enforce.contains("Usage: criv enforce --stage <STAGE>"));
        assert!(!enforce.contains("<--stage <STAGE>>"));
    }
}

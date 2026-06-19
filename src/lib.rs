mod c4;
mod c4_code;
mod check;
mod config;
mod enforce;
mod init;
mod query;
mod search;
mod source_graph;
mod source_index;
mod state;
mod structural;
mod util;
mod vault;
mod watch;

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
    after_help = "Implemented query names: next-adr-id, targets, references, cites, cited-by, governs, governing, coverage, nodes, callers, callees, attack-surface, c4-elements, c4-relationships, c4-code, diff, orphan-docs."
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
    Check(check::CheckOptions),
    Query(query::QueryOptions),
    Search(search::SearchOptions),
    Watch(watch::WatchOptions),
    Enforce(enforce::EnforceOptions),
}

pub fn run(args: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    if let Some(help) = usage_help(&args) {
        print!("{help}");
        return Ok(());
    }

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
        Some(Command::Init(options)) => init::run(&cwd, options),
        Some(Command::Check(options)) => check::run(&cwd, options),
        Some(Command::Query(options)) => query::run(&cwd, options),
        Some(Command::Search(options)) => search::run(&cwd, options),
        Some(Command::Watch(options)) => watch::run(&cwd, options),
        Some(Command::Enforce(options)) => enforce::run(&cwd, options),
    }
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
    use super::{usage_help, write_usage_spec};

    #[test]
    fn usage_spec_includes_criv_commands() {
        let mut output = Vec::new();
        write_usage_spec(&mut output);

        let spec = String::from_utf8(output).expect("usage spec should be utf-8");

        assert!(spec.contains("bin criv"));
        assert!(spec.contains("flag --usage hide=#true"));
        assert!(spec.contains("cmd check"));
        assert!(spec.contains("cmd query"));
        assert!(spec.contains("cmd enforce"));
    }

    #[test]
    fn usage_help_renders_root_and_subcommand_help() {
        let root = usage_help(&["help".to_string()]).expect("root help should render");
        let default = usage_help(&[]).expect("default help should render");
        let query = usage_help(&["help".to_string(), "query".to_string()])
            .expect("query help should render");

        assert!(root.contains("Usage: criv"));
        assert_eq!(root, default);
        assert!(root.contains("Commands:"));
        assert!(!root.contains("--usage"));
        assert!(query.contains("Usage: criv query"));
        assert!(query.contains("[OPTIONS]"));
        assert!(query.contains("--without-docs"));
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

//! The `--usage-json` command tree: visible commands, arguments, and flags.

use std::io::Write;

use usage_rs::spec::CommandMeta;

use crate::{CrivError, Result};

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

fn json_command<'a>(meta: &'a CommandMeta<'a>, parent: &str) -> JsonCommand<'a> {
    let path = if parent.is_empty() {
        meta.cmd().name.to_string()
    } else {
        format!("{parent} {}", meta.cmd().name)
    };
    JsonCommand {
        name: meta.cmd().name,
        about: meta.about(),
        args: meta
            .args()
            .iter()
            .filter(|arg| !arg.hide())
            .map(|arg| JsonArg {
                name: arg.arg().name,
                required: arg.required(),
                help: arg.help(),
                choices: arg.choices(),
                default: arg.default(),
            })
            .collect(),
        flags: meta
            .flags()
            .iter()
            .filter(|flag| !flag.hide())
            .map(|flag| JsonFlag {
                name: flag.flag().name,
                long: flag.flag().longs,
                required: flag.required(),
                takes_value: flag.flag().takes_value,
                help: flag.help(),
                choices: flag.choices(),
                default: flag.default(),
                conflicts: flag.conflicts(),
                requires: flag.requires(),
            })
            .collect(),
        subcommands: meta
            .subcommands()
            .iter()
            .filter(|child| !child.hide())
            .map(|child| json_command(child, &path))
            .collect(),
        path,
    }
}

/// Writes the visible command tree under `root` as pretty JSON.
pub fn write(root: &CommandMeta<'_>, writer: &mut dyn Write) -> Result<()> {
    let json = serde_json::to_string_pretty(&json_command(root, ""))
        .map_err(|error| CrivError::new(format!("failed to serialize command tree: {error}")))?;
    writeln!(writer, "{json}")?;
    Ok(())
}

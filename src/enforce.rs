use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args as ClapArgs, ValueEnum};

use crate::check;
use crate::search;
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Stage {
    Commit,
    Push,
    Ci,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct EnforceOptions {
    #[arg(long, value_enum)]
    stage: Stage,
}

pub(crate) fn run(root: &Path, options: EnforceOptions) -> Result<()> {
    let vault = Vault::load(root)?;
    if !vault
        .config
        .enforce_stages
        .iter()
        .any(|stage| stage == options.stage.as_str())
    {
        return Err(CrivError::new(format!(
            "stage `{}` is not enabled in criv.toml",
            options.stage.as_str()
        )));
    }

    let diagnostics = check::validate(&vault);
    let errors = diagnostics.iter().filter(|diag| diag.is_error()).count();
    let warnings = diagnostics.iter().filter(|diag| diag.is_warning()).count();

    let changed_files = changed_files(root, options.stage);
    let violations = policy_violations(root, &vault, changed_files.as_ref())?;
    let tool_files = enforcement_files(&vault, changed_files.as_ref());
    let tool_errors = run_native_tools(root, &tool_files)?;

    match options.stage {
        Stage::Commit => {
            println!(
                "commit enforcement: {errors} validation errors, {warnings} warnings, {} staged files",
                changed_files.as_ref().map_or(0, Vec::len)
            );
        }
        Stage::Push => {
            println!(
                "push enforcement: {errors} validation errors, {warnings} warnings, {} changed files",
                changed_files.as_ref().map_or(0, Vec::len)
            );
        }
        Stage::Ci => {
            println!("ci enforcement: {errors} validation errors, {warnings} warnings");
        }
    }

    if !violations.is_empty() {
        for violation in &violations {
            println!("{violation}");
        }
        return Err(CrivError::new(format!(
            "{} policy violation(s) found",
            violations.len()
        )));
    }

    if errors > 0 {
        return Err(CrivError::new("enforcement failed"));
    }
    if tool_errors > 0 {
        return Err(CrivError::new(format!(
            "{} native enforcement tool(s) failed",
            tool_errors
        )));
    }

    println!("enforcement passed");
    Ok(())
}

fn policy_violations(
    root: &Path,
    vault: &Vault,
    changed_files: Option<&Vec<String>>,
) -> Result<Vec<String>> {
    let mut violations = Vec::new();
    for note in &vault.notes {
        if note.status.as_deref() != Some("accepted") {
            continue;
        }
        let Some(adr_id) = &note.id else {
            continue;
        };
        let scopes = vault.effective_governs(note);
        for pattern in &note.policy_pattern_ids {
            let pattern_id = format!("{adr_id}/{pattern}");
            let rows = if let Some(pattern_def) = vault.config.pattern_defs.get(&pattern_id) {
                if let Some(pattern_body) = pattern_def.lexical_pattern() {
                    search::search_lexical_pattern(root, vault, pattern_body, &scopes)?
                } else {
                    Vec::new()
                }
            } else {
                search::search_lexical_pattern(root, vault, pattern, &scopes)?
            };
            for row in rows {
                if changed_files.is_some_and(|files| !files.contains(&row.path)) {
                    continue;
                }
                let line = row.line.map(|line| format!(":{line}")).unwrap_or_default();
                violations.push(format!(
                    "{}{}: {} policy `{pattern_id}` matched `{}`",
                    row.path, line, adr_id, row.text
                ));
            }
        }
    }
    Ok(violations)
}

fn changed_files(root: &Path, stage: Stage) -> Option<Vec<String>> {
    match stage {
        Stage::Commit => git_changed_files(root, &["diff", "--name-only", "--cached"]),
        Stage::Push => git_changed_files(root, &["diff", "--name-only", "@{upstream}...HEAD"])
            .or_else(|| git_changed_files(root, &["diff", "--name-only", "HEAD~1..HEAD"])),
        Stage::Ci => None,
    }
}

fn git_changed_files(root: &Path, args: &[&str]) -> Option<Vec<String>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn enforcement_files(vault: &Vault, changed_files: Option<&Vec<String>>) -> Vec<String> {
    vault
        .source_files()
        .iter()
        .filter(|path| changed_files.is_none_or(|files| files.contains(path)))
        .cloned()
        .collect()
}

fn run_native_tools(root: &Path, files: &[String]) -> Result<usize> {
    let js_ts = files
        .iter()
        .filter(|path| matches_extension(path, &["js", "jsx", "ts", "tsx", "mjs", "cjs"]))
        .cloned()
        .collect::<Vec<_>>();
    let python = files
        .iter()
        .filter(|path| matches_extension(path, &["py"]))
        .cloned()
        .collect::<Vec<_>>();

    let mut failures = 0;
    failures += run_optional_tool(
        root,
        "ESLint",
        local_or_path(root, "node_modules/.bin/eslint", "eslint"),
        &js_ts,
    )?;
    failures += run_optional_tool(
        root,
        "Ruff",
        local_or_path(root, ".venv/bin/ruff", "ruff"),
        &python,
    )?;
    Ok(failures)
}

fn run_optional_tool(
    root: &Path,
    label: &str,
    command: ToolCommand,
    files: &[String],
) -> Result<usize> {
    if files.is_empty() {
        return Ok(0);
    }

    let mut process = Command::new(command.program());
    process.current_dir(root);
    if label == "Ruff" {
        process.arg("check");
    }
    process.args(files);

    match process.output() {
        Ok(output) if output.status.success() => {
            println!("{label}: checked {} file(s)", files.len());
            Ok(0)
        }
        Ok(output) => {
            println!("{label}: failed");
            print_tool_output(&output.stdout);
            print_tool_output(&output.stderr);
            Ok(1)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("{label}: skipped {} file(s); tool not found", files.len());
            Ok(0)
        }
        Err(err) => Err(CrivError::new(format!("failed to run {label}: {err}"))),
    }
}

fn print_tool_output(bytes: &[u8]) {
    let output = String::from_utf8_lossy(bytes);
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        println!("{line}");
    }
}

fn local_or_path(root: &Path, local: &str, fallback: &'static str) -> ToolCommand {
    let local = root.join(local);
    if local.exists() {
        ToolCommand::Path(local)
    } else {
        ToolCommand::Name(fallback)
    }
}

enum ToolCommand {
    Path(PathBuf),
    Name(&'static str),
}

impl ToolCommand {
    fn program(&self) -> &std::ffi::OsStr {
        match self {
            Self::Path(path) => path.as_os_str(),
            Self::Name(name) => std::ffi::OsStr::new(name),
        }
    }
}

fn matches_extension(path: &str, extensions: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extensions.contains(&extension))
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Commit => "commit",
            Stage::Push => "push",
            Stage::Ci => "ci",
        }
    }
}

use std::path::Path;
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

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Commit => "commit",
            Stage::Push => "push",
            Stage::Ci => "ci",
        }
    }
}

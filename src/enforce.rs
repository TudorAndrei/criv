use std::path::Path;

use crate::check;
use crate::vault::Vault;
use crate::{Args, CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Commit,
    Push,
    Ci,
}

#[derive(Debug)]
pub(crate) struct EnforceOptions {
    stage: Stage,
}

impl EnforceOptions {
    pub(crate) fn parse(mut args: Args) -> Result<Self> {
        let mut stage = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--stage" => {
                    let value = args.expect_value("--stage")?;
                    stage = Some(match value.as_str() {
                        "commit" => Stage::Commit,
                        "push" => Stage::Push,
                        "ci" => Stage::Ci,
                        _ => {
                            return Err(CrivError::usage(format!(
                                "unknown enforce stage `{value}`"
                            )));
                        }
                    });
                }
                other => {
                    return Err(CrivError::usage(format!(
                        "unknown enforce option `{other}`"
                    )));
                }
            }
        }

        Ok(Self {
            stage: stage.ok_or_else(|| {
                CrivError::usage("missing enforce stage; use --stage commit|push|ci")
            })?,
        })
    }
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

    let accepted_policy_count = vault
        .notes
        .iter()
        .filter(|note| note.status.as_deref() == Some("accepted"))
        .map(|note| note.policy_pattern_ids.len())
        .sum::<usize>();

    match options.stage {
        Stage::Commit => {
            println!("commit enforcement: {errors} validation errors, {warnings} warnings");
        }
        Stage::Push => {
            println!("push enforcement: {errors} validation errors, {warnings} warnings");
        }
        Stage::Ci => {
            println!("ci enforcement: {errors} validation errors, {warnings} warnings");
        }
    }

    if accepted_policy_count > 0 {
        return Err(CrivError::new(format!(
            "{} accepted ADR policy pattern(s) require the ast-grep backend, which is not linked yet",
            accepted_policy_count
        )));
    }

    if errors > 0 {
        return Err(CrivError::new("enforcement failed"));
    }

    println!("enforcement passed");
    Ok(())
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

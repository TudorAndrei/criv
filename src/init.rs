pub(crate) mod templates;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use clap::Args as ClapArgs;

use crate::generated_skills::{self, InstallMode, SkillPublication};
use crate::util::{append_line_if_missing_in, create_dir_in, write_atomic_in, write_new_in};
use crate::{CrivError, Result};

const VSCODE_EXTENSION_ID: &str = "criv.vscode-criv";
const VSCODE_EXTENSIONS_JSON: &str = ".vscode/extensions.json";

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct InitOptions {
    #[arg(long)]
    no_obsidian: bool,
    #[arg(long)]
    no_vscode: bool,
    #[arg(long)]
    no_skills: bool,
    #[arg(long)]
    force_skills: bool,
}

pub(crate) fn run(root: &Path, options: InitOptions) -> Result<()> {
    // Resolve once up front, the way `install_git_hooks` already does. Every
    // confined write canonicalizes its root, so doing it here keeps scaffolding
    // from repeating the syscall for each of the ~20 templates, and pins one
    // resolved root for the whole run.
    let root = &fs::canonicalize(root).map_err(|err| {
        CrivError::new(format!(
            "failed to resolve criv root {}: {err}",
            root.display()
        ))
    })?;
    let mut created = Vec::new();
    let mut refreshed = Vec::new();
    let mut link_messages = Vec::new();

    if options.force_skills {
        if !options.no_skills {
            let report = generated_skills::install(root, InstallMode::Refresh)?;
            collect_skill_publications(&report, &mut created, &mut refreshed);
            link_messages.push(
                generated_skills::describe_claude_publication(report.claude_publication())
                    .to_string(),
            );
        }
        print_init_result(created, refreshed, link_messages);
        return Ok(());
    }

    write_template(
        root,
        "criv.toml",
        &templates::default_config()?,
        &mut created,
    )?;

    create_dir_in(root, Path::new("docs/adr"))?;
    create_dir_in(root, Path::new(".criv/snapshots"))?;

    write_template(
        root,
        ".criv/state.json",
        &templates::default_state()?,
        &mut created,
    )?;
    write_template(
        root,
        "docs/adr/README.md",
        &templates::adr_readme()?,
        &mut created,
    )?;

    if !options.no_skills {
        let report = generated_skills::install(root, InstallMode::CreateOnly)?;
        collect_skill_publications(&report, &mut created, &mut refreshed);
        link_messages.push(
            generated_skills::describe_claude_publication(report.claude_publication()).to_string(),
        );
    }

    if !options.no_obsidian {
        for template in templates::obsidian_plugin()? {
            write_template(
                root,
                template.path,
                template.contents.as_ref(),
                &mut created,
            )?;
        }
    }

    if !options.no_vscode {
        write_vscode_extension_recommendation(root, &mut created)?;
    }

    append_line_if_missing_in(root, Path::new("."), Path::new(".gitignore"), ".criv/")?;

    print_init_result(created, refreshed, link_messages);

    Ok(())
}

fn print_init_result(
    created: Vec<&'static str>,
    refreshed: Vec<&'static str>,
    link_messages: Vec<String>,
) {
    if created.is_empty() && refreshed.is_empty() {
        println!("criv vault already initialized");
    } else {
        println!("initialized criv vault");
        for path in created {
            println!("created {path}");
        }
        for path in refreshed {
            println!("refreshed {path}");
        }
    }
    for message in link_messages {
        println!("{message}");
    }
}

fn collect_skill_publications(
    report: &generated_skills::InstallReport,
    created: &mut Vec<&'static str>,
    refreshed: &mut Vec<&'static str>,
) {
    for fact in report.skill_publications() {
        match fact.publication {
            SkillPublication::Created => created.push(fact.path),
            SkillPublication::Refreshed => refreshed.push(fact.path),
            SkillPublication::Preserved => {}
        }
    }
}

fn write_vscode_extension_recommendation(
    root: &Path,
    created: &mut Vec<&'static str>,
) -> Result<()> {
    let path = root.join(VSCODE_EXTENSIONS_JSON);
    if !path.exists() {
        let value = serde_json::json!({
            "recommendations": [VSCODE_EXTENSION_ID],
        });
        write_atomic_in(
            root,
            Path::new("."),
            Path::new(VSCODE_EXTENSIONS_JSON),
            &json_pretty(&value, VSCODE_EXTENSIONS_JSON)?,
        )?;
        created.push(VSCODE_EXTENSIONS_JSON);
        return Ok(());
    }

    let contents = fs::read_to_string(&path)?;
    let mut value: serde_json::Value = serde_json::from_str(&contents).map_err(|err| {
        CrivError::new(format!(
            "failed to parse {}: {err}",
            root.join(VSCODE_EXTENSIONS_JSON).display()
        ))
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        CrivError::new(format!(
            "{} must be a JSON object",
            root.join(VSCODE_EXTENSIONS_JSON).display()
        ))
    })?;
    let recommendations = object
        .entry("recommendations")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let recommendations = recommendations.as_array_mut().ok_or_else(|| {
        CrivError::new(format!(
            "{} recommendations must be a JSON array",
            root.join(VSCODE_EXTENSIONS_JSON).display()
        ))
    })?;

    if recommendations
        .iter()
        .any(|value| value.as_str() == Some(VSCODE_EXTENSION_ID))
    {
        return Ok(());
    }

    recommendations.push(serde_json::Value::String(VSCODE_EXTENSION_ID.to_string()));
    write_atomic_in(
        root,
        Path::new("."),
        Path::new(VSCODE_EXTENSIONS_JSON),
        &json_pretty(&value, VSCODE_EXTENSIONS_JSON)?,
    )?;
    Ok(())
}

fn json_pretty(value: &impl serde::Serialize, label: &str) -> Result<String> {
    let mut json = serde_json::to_string_pretty(value)
        .map_err(|err| CrivError::new(format!("failed to serialize {label}: {err}")))?;
    json.push('\n');
    Ok(json)
}

fn write_template(
    root: &Path,
    path: &'static str,
    contents: &str,
    created: &mut Vec<&'static str>,
) -> Result<()> {
    // Scaffolding lands all over the repository, so the scope is the root
    // itself. Per ADR-0044 that still enforces root confinement, symlink
    // rejection, and relative-path validation.
    if write_new_in(root, Path::new("."), Path::new(path), contents)? {
        created.push(path);
    }
    Ok(())
}

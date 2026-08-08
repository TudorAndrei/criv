pub(crate) mod templates;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use clap::Args as ClapArgs;

use crate::generated_skills::{self, InstallMode, SkillPublication};
use crate::util::{append_line_if_missing_in, create_dir_in, write_new_in};
use crate::{CrivError, Result};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct InitOptions {
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

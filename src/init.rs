pub mod templates;

#[cfg(test)]
mod tests;

use std::path::Path;

use usage::Args as UsageArgs;

use crate::Result;
use crate::install::{self, InstallMode, SkillPublication};
use crate::repository::RepositoryFiles;

#[derive(Debug, Default, UsageArgs)]
pub struct InitOptions {
    #[usage(long)]
    no_skills: bool,
    #[usage(long)]
    force_skills: bool,
}

pub fn run(root: &Path, options: InitOptions) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    let scope = files.write_scope(Path::new("."))?;
    let mut created = Vec::new();
    let mut refreshed = Vec::new();
    let mut link_messages = Vec::new();

    if options.force_skills {
        if !options.no_skills {
            let report = install::install_skills_from(&files, InstallMode::Refresh)?;
            collect_skill_publications(&report, &mut created, &mut refreshed);
            link_messages.push(
                install::describe_claude_publication(report.claude_publication()).to_string(),
            );
        }
        print_init_result(created, refreshed, link_messages);
        return Ok(());
    }

    write_template(
        &scope,
        "criv.toml",
        &templates::default_config(),
        &mut created,
    )?;

    scope.create_dir(Path::new("docs/adr"))?;
    scope.create_dir(Path::new(".criv/snapshots"))?;

    write_template(
        &scope,
        ".criv/state.json",
        &templates::default_state()?,
        &mut created,
    )?;
    write_template(
        &scope,
        "docs/adr/README.md",
        &templates::adr_readme()?,
        &mut created,
    )?;

    if !options.no_skills {
        let report = install::install_skills_from(&files, InstallMode::CreateOnly)?;
        collect_skill_publications(&report, &mut created, &mut refreshed);
        link_messages
            .push(install::describe_claude_publication(report.claude_publication()).to_string());
    }

    scope.append_line_if_missing(Path::new(".gitignore"), ".criv/")?;

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
    report: &install::InstallReport,
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
    scope: &crate::repository::RepositoryWriteScope<'_>,
    path: &'static str,
    contents: &str,
    created: &mut Vec<&'static str>,
) -> Result<()> {
    // Scaffolding lands all over the repository, so the scope is the root
    // itself. Per ADR-0044 that still enforces root confinement, symlink
    // rejection, and relative-path validation.
    if scope.write_new(Path::new(path), contents)? {
        created.push(path);
    }
    Ok(())
}

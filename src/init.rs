pub mod templates;

#[cfg(test)]
mod tests;

use std::path::Path;

use criv_state_wire::layout;
use usage_rs::Args as UsageArgs;

use crate::Result;
use crate::config::CONFIG_FILE;
use crate::install::{self, InstallMode, SkillPublication};
use crate::repository::RepositoryFiles;

#[derive(Debug, Default, UsageArgs)]
pub struct InitOptions {
    #[usage(long)]
    no_skills: bool,
    #[usage(long)]
    force_skills: bool,
}

pub fn run(root: &Path, options: &InitOptions) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    let scope = files.write_scope(Path::new("."))?;
    let mut created = Vec::new();
    let mut refreshed = Vec::new();

    if !options.force_skills {
        scope.create_dir(Path::new("docs/adr"))?;
        scope.create_dir(Path::new(layout::SNAPSHOTS))?;
        for (path, contents) in [
            (CONFIG_FILE, templates::DEFAULT_CONFIG.to_owned()),
            (layout::STATE, templates::default_state()?),
            ("docs/adr/README.md", templates::adr_readme()?),
        ] {
            if scope.write_new(Path::new(path), &contents)? {
                created.push(path.to_owned());
            }
        }
    }

    let claude_message = if options.no_skills {
        None
    } else {
        let mode = if options.force_skills {
            InstallMode::Refresh
        } else {
            InstallMode::CreateOnly
        };
        let report = install::install_skills(&files, mode)?;
        for fact in report.skill_publications() {
            match fact.publication {
                SkillPublication::Created => created.push(fact.path.clone()),
                SkillPublication::Refreshed => refreshed.push(fact.path.clone()),
                SkillPublication::Preserved => {}
            }
        }
        Some(install::describe_claude_publication(
            report.claude_publication(),
        ))
    };

    if !options.force_skills {
        scope.append_line_if_missing(Path::new(".gitignore"), &format!("{}/", layout::DIR))?;
    }

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
    if let Some(message) = claude_message {
        println!("{message}");
    }

    Ok(())
}

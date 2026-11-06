pub mod templates;

#[cfg(test)]
mod tests;

use std::path::Path;

use criv_state_wire::layout;
use usage_rs::Args as UsageArgs;

use crate::Result;
use crate::config::{CONFIG_FILE, Config};
use crate::install::{self, InstallMode, SkillPublication};
use crate::repository::RepositoryFiles;

#[derive(Debug, Default, UsageArgs)]
pub struct InitOptions {
    #[usage(long, conflicts = "--force-skills")]
    no_skills: bool,
    #[usage(long)]
    force_skills: bool,
}

/// The paths `criv init` wrote, in report order.
#[derive(Default)]
struct Written {
    created: Vec<String>,
    refreshed: Vec<String>,
}

pub fn run(root: &Path, options: &InitOptions) -> Result<()> {
    let files = RepositoryFiles::open(root)?;
    let mut written = Written::default();
    let claude_message = if options.force_skills {
        Some(publish_skills(&files, InstallMode::Refresh, &mut written)?)
    } else {
        let scope = files.write_scope(Path::new("."))?;
        let config = Config::default();
        scope.create_dir(Path::new(&config.adr_prefix()))?;
        scope.create_dir(Path::new(layout::SNAPSHOTS))?;
        for (path, contents) in [
            (CONFIG_FILE.to_owned(), templates::DEFAULT_CONFIG.to_owned()),
            (layout::STATE.to_owned(), templates::default_state()?),
            (config.adr_index_path(), templates::adr_readme()?),
        ] {
            if scope.write_new(Path::new(&path), &contents)? {
                written.created.push(path);
            }
        }
        let message = (!options.no_skills)
            .then(|| publish_skills(&files, InstallMode::CreateOnly, &mut written))
            .transpose()?;
        scope.append_line_if_missing(Path::new(".gitignore"), &format!("{}/", layout::DIR))?;
        message
    };

    if written.created.is_empty() && written.refreshed.is_empty() {
        println!("criv vault already initialized");
    } else {
        println!("initialized criv vault");
        for path in written.created {
            println!("created {path}");
        }
        for path in written.refreshed {
            println!("refreshed {path}");
        }
    }
    if let Some(message) = claude_message {
        println!("{message}");
    }

    Ok(())
}

/// Publish the agent skills and return the `.claude/skills` message.
fn publish_skills(
    files: &RepositoryFiles,
    mode: InstallMode,
    written: &mut Written,
) -> Result<&'static str> {
    let report = install::install_skills(files, mode)?;
    for fact in report.skill_publications() {
        match fact.publication {
            SkillPublication::Created => written.created.push(fact.path.clone()),
            SkillPublication::Refreshed => written.refreshed.push(fact.path.clone()),
            SkillPublication::Preserved => {}
        }
    }
    Ok(report.claude_message())
}

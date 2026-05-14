mod templates;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use clap::Args as ClapArgs;

use crate::Result;
use crate::util::{append_line_if_missing, write_new};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct InitOptions {
    #[arg(long)]
    no_obsidian: bool,
    #[arg(long)]
    no_skills: bool,
}

pub(crate) fn run(root: &Path, options: InitOptions) -> Result<()> {
    let mut created = Vec::new();

    write_template(
        root,
        "criv.toml",
        &templates::default_config()?,
        &mut created,
    )?;

    fs::create_dir_all(root.join("docs/adr"))?;
    fs::create_dir_all(root.join(".criv/snapshots"))?;

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
        write_templates(root, templates::agent_skills(), &mut created)?;
        write_templates(root, templates::claude_skills(), &mut created)?;
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

    append_line_if_missing(&root.join(".gitignore"), ".criv/")?;

    if created.is_empty() {
        println!("criv vault already initialized");
    } else {
        println!("initialized criv vault");
        for path in created {
            println!("created {path}");
        }
    }

    Ok(())
}

fn write_templates(
    root: &Path,
    templates: &[templates::StaticTemplate],
    created: &mut Vec<&'static str>,
) -> Result<()> {
    for template in templates {
        write_template(root, template.path, template.contents, created)?;
    }
    Ok(())
}

fn write_template(
    root: &Path,
    path: &'static str,
    contents: &str,
    created: &mut Vec<&'static str>,
) -> Result<()> {
    if write_new(&root.join(path), contents)? {
        created.push(path);
    }
    Ok(())
}

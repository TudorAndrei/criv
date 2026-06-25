mod templates;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use clap::Args as ClapArgs;
use git2::{ErrorCode, Repository};

use crate::util::{append_line_if_missing, normalize_rel, write_atomic, write_new};
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
    no_hooks: bool,
    #[arg(long)]
    force_hooks: bool,
}

pub(crate) fn run(root: &Path, options: InitOptions) -> Result<()> {
    let mut created = Vec::new();
    let mut hook_messages = Vec::new();

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

    if !options.no_vscode {
        write_vscode_extension_recommendation(root, &mut created)?;
    }

    append_line_if_missing(&root.join(".gitignore"), ".criv/")?;

    if !options.no_hooks {
        hook_messages = install_git_hooks(root, options.force_hooks)?;
    }

    if created.is_empty() {
        println!("criv vault already initialized");
    } else {
        println!("initialized criv vault");
        for path in created {
            println!("created {path}");
        }
    }
    for message in hook_messages {
        println!("{message}");
    }

    Ok(())
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
        write_atomic(&path, &json_pretty(&value, VSCODE_EXTENSIONS_JSON)?)?;
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
    write_atomic(&path, &json_pretty(&value, VSCODE_EXTENSIONS_JSON)?)?;
    Ok(())
}

fn json_pretty(value: &impl serde::Serialize, label: &str) -> Result<String> {
    let mut json = serde_json::to_string_pretty(value)
        .map_err(|err| CrivError::new(format!("failed to serialize {label}: {err}")))?;
    json.push('\n');
    Ok(json)
}

fn install_git_hooks(root: &Path, force: bool) -> Result<Vec<String>> {
    let Some(repo) = discover_worktree(root)? else {
        return Ok(vec![
            "skipped Git hooks: not inside a Git repository".to_string(),
        ]);
    };

    let Some(workdir) = repo.workdir() else {
        return Ok(vec![
            "skipped Git hooks: bare repositories do not have a worktree".to_string(),
        ]);
    };

    let workdir = fs::canonicalize(workdir)?;
    let root = fs::canonicalize(root)?;
    let relative_root = repo_relative_root(&workdir, &root)?;
    let hooks_dir = workdir.join(".githooks");
    fs::create_dir_all(&hooks_dir)?;

    let messages = vec![
        write_hook(
            &hooks_dir.join("pre-commit"),
            &templates::pre_commit_hook(&relative_root),
            force,
        )?,
        write_hook(
            &hooks_dir.join("pre-push"),
            &templates::pre_push_hook(&relative_root),
            force,
        )?,
        configure_hooks_path(&repo, force)?,
    ];

    Ok(messages)
}

fn discover_worktree(root: &Path) -> Result<Option<Repository>> {
    match Repository::discover(root) {
        Ok(repo) => Ok(Some(repo)),
        Err(err) if err.code() == ErrorCode::NotFound => Ok(None),
        Err(err) => Err(CrivError::new(format!(
            "failed to discover Git repository: {err}"
        ))),
    }
}

fn repo_relative_root(workdir: &Path, root: &Path) -> Result<String> {
    if root == workdir {
        return Ok(".".to_string());
    }
    let relative = root.strip_prefix(workdir).map_err(|_| {
        CrivError::new(format!(
            "criv root `{}` is outside Git worktree `{}`",
            root.display(),
            workdir.display()
        ))
    })?;
    Ok(normalize_rel(relative))
}

fn write_hook(path: &Path, contents: &str, force: bool) -> Result<String> {
    let hook_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("hook");
    if path.exists() && !force {
        return Ok(format!(
            "skipped Git hook .githooks/{hook_name}: already exists"
        ));
    }
    let existed = path.exists();
    fs::write(path, contents)?;
    set_executable(path)?;
    let action = if existed { "wrote" } else { "created" };
    Ok(format!("{action} Git hook .githooks/{hook_name}"))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn configure_hooks_path(repo: &Repository, force: bool) -> Result<String> {
    let mut config = repo
        .config()
        .map_err(|err| CrivError::new(format!("failed to open Git configuration: {err}")))?;
    match config.get_string("core.hooksPath") {
        Ok(value) if value == ".githooks" => {
            Ok("Git core.hooksPath already set to .githooks".to_string())
        }
        Ok(value) if !force => Ok(format!(
            "skipped Git core.hooksPath: already set to `{value}`"
        )),
        Ok(_) | Err(_) => {
            config
                .set_str("core.hooksPath", ".githooks")
                .map_err(|err| {
                    CrivError::new(format!("failed to set Git core.hooksPath: {err}"))
                })?;
            Ok("configured Git core.hooksPath=.githooks".to_string())
        }
    }
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

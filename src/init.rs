mod templates;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use clap::Args as ClapArgs;

use crate::util::{
    append_line_if_missing_in, create_dir_in, normalize_rel, write_atomic_in, write_new_in,
};
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

    append_line_if_missing_in(root, Path::new("."), Path::new(".gitignore"), ".criv/")?;

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

fn install_git_hooks(root: &Path, force: bool) -> Result<Vec<String>> {
    let Some(discovery) = discover_worktree(root)? else {
        return Ok(vec![
            "skipped Git hooks: not inside a Git repository".to_string(),
        ]);
    };

    let GitDiscovery::Worktree(workdir) = discovery else {
        return Ok(vec![
            "skipped Git hooks: bare repositories do not have a worktree".to_string(),
        ]);
    };

    let workdir = fs::canonicalize(workdir)?;
    let root = fs::canonicalize(root)?;
    let relative_root = repo_relative_root(&workdir, &root)?;
    // Hooks live in the Git worktree, which may sit above the criv root, so the
    // worktree is the confinement root here; `.githooks` is the allowed scope.
    create_dir_in(&workdir, Path::new(".githooks"))?;

    let messages = vec![
        write_hook(
            &workdir,
            "pre-commit",
            &templates::pre_commit_hook(&relative_root),
            force,
        )?,
        write_hook(
            &workdir,
            "pre-push",
            &templates::pre_push_hook(&relative_root),
            force,
        )?,
        configure_hooks_path(&workdir, force)?,
    ];

    Ok(messages)
}

enum GitDiscovery {
    Worktree(PathBuf),
    Bare,
}

fn discover_worktree(root: &Path) -> Result<Option<GitDiscovery>> {
    let bare = git_output(root, &["rev-parse", "--is-bare-repository"])?;
    if !bare.status.success() {
        if bare.status.code() == Some(128) {
            return Ok(None);
        }
        return Err(git_command_error(
            "failed to discover Git repository",
            &bare,
        ));
    }

    match bare.stdout.trim() {
        "true" => Ok(Some(GitDiscovery::Bare)),
        "false" => {
            let top_level = git_output(root, &["rev-parse", "--show-toplevel"])?;
            if !top_level.status.success() {
                return Err(git_command_error(
                    "failed to discover Git repository",
                    &top_level,
                ));
            }
            let path = top_level.stdout.trim();
            if path.is_empty() {
                return Err(CrivError::new(
                    "failed to discover Git repository: git returned an empty worktree path",
                ));
            }
            Ok(Some(GitDiscovery::Worktree(PathBuf::from(path))))
        }
        value => Err(CrivError::new(format!(
            "failed to discover Git repository: unexpected --is-bare-repository output `{value}`"
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

fn write_hook(workdir: &Path, hook_name: &str, contents: &str, force: bool) -> Result<String> {
    let destination = PathBuf::from(".githooks").join(hook_name);
    let path = workdir.join(&destination);
    if path.exists() && !force {
        return Ok(format!(
            "skipped Git hook .githooks/{hook_name}: already exists"
        ));
    }
    let existed = path.exists();
    write_atomic_in(workdir, Path::new(".githooks"), &destination, contents)?;
    set_executable(&path)?;
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

fn configure_hooks_path(workdir: &Path, force: bool) -> Result<String> {
    let config = git_output(workdir, &["config", "core.hooksPath"])?;
    match (config.status.code(), config.stdout.trim()) {
        (Some(0), ".githooks") => Ok("Git core.hooksPath already set to .githooks".to_string()),
        (Some(0), value) if !force => Ok(format!(
            "skipped Git core.hooksPath: already set to `{value}`"
        )),
        (Some(0), _) | (Some(1), _) => {
            let result = git_output(workdir, &["config", "core.hooksPath", ".githooks"])?;
            if !result.status.success() {
                return Err(git_command_error(
                    "failed to set Git core.hooksPath",
                    &result,
                ));
            }
            Ok("configured Git core.hooksPath=.githooks".to_string())
        }
        _ => Err(git_command_error(
            "failed to read Git core.hooksPath",
            &config,
        )),
    }
}

struct GitResult {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn git_output(root: &Path, args: &[&str]) -> Result<GitResult> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|err| CrivError::new(format!("failed to run git: {err}")))?;
    Ok(GitResult {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn git_command_error(context: &str, result: &GitResult) -> CrivError {
    let detail = result.stderr.trim();
    let detail = if detail.is_empty() {
        result.stdout.trim()
    } else {
        detail
    };
    if detail.is_empty() {
        CrivError::new(format!("{context}: git exited with {}", result.status))
    } else {
        CrivError::new(format!("{context}: {detail}"))
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
    // Scaffolding lands all over the repository, so the scope is the root
    // itself. Per ADR-0044 that still enforces root confinement, symlink
    // rejection, and relative-path validation.
    if write_new_in(root, Path::new("."), Path::new(path), contents)? {
        created.push(path);
    }
    Ok(())
}

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(windows)]
use std::ffi::OsStr;

use clap::{Args as ClapArgs, ValueEnum};

use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Editor {
    Code,
    Cursor,
}

impl Editor {
    fn command_name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Cursor => "cursor",
        }
    }

    fn alternative(self) -> &'static str {
        match self {
            Self::Code => "cursor",
            Self::Cursor => "code",
        }
    }
}

#[derive(Debug, ClapArgs)]
pub(crate) struct InstallEditorOptions {
    /// Editor CLI to use.
    #[arg(long, value_enum)]
    editor: Editor,
    /// Validate the bundled viewer and show the install command without changing the editor.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn run(options: InstallEditorOptions) -> Result<()> {
    let vsix = bundled_vsix()?;
    let editor_cli = resolve_editor(options.editor)?;

    if options.dry_run {
        println!(
            "editor install dry run: {:?} --install-extension {:?}",
            editor_cli, vsix
        );
        return Ok(());
    }

    let output = run_install(&editor_cli, &vsix).map_err(|error| {
        CrivError::new(format!(
            "editor install failed: could not start {}: {error}",
            options.editor.command_name()
        ))
    })?;
    if !output.status.success() {
        return Err(editor_failure(options.editor, &output));
    }

    println!(
        "installed the criv viewer in {} from {}",
        options.editor.command_name(),
        vsix.display()
    );
    Ok(())
}

fn bundled_vsix() -> Result<PathBuf> {
    let executable = env::current_exe().map_err(|error| {
        CrivError::new(format!(
            "editor install failed: could not locate the criv executable: {error}"
        ))
    })?;
    let directory = executable.parent().ok_or_else(|| {
        CrivError::new("editor install failed: the criv executable has no parent directory")
    })?;
    let candidate = directory.join("vscode-criv.vsix");
    let metadata = fs::metadata(&candidate).map_err(|error| {
        CrivError::new(format!(
            "editor install failed: bundled viewer is missing: {}: {error}; reinstall criv from a release archive",
            candidate.display(),
        ))
    })?;
    if !metadata.is_file() {
        return Err(CrivError::new(format!(
            "editor install failed: bundled viewer is not a file: {}; reinstall criv from a release archive",
            candidate.display()
        )));
    }
    fs::canonicalize(&candidate).map_err(|error| {
        CrivError::new(format!(
            "editor install failed: could not resolve bundled viewer {}: {error}",
            candidate.display()
        ))
    })
}

fn resolve_editor(editor: Editor) -> Result<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    for directory in env::split_paths(&path).filter(|directory| !directory.as_os_str().is_empty()) {
        for candidate in editor_candidates(editor) {
            let executable = directory.join(candidate);
            if executable.is_file() {
                return Ok(executable);
            }
        }
    }
    Err(CrivError::new(format!(
        "editor install failed: {} was not found on PATH; install its shell command or use --editor {}",
        editor.command_name(),
        editor.alternative()
    )))
}

#[cfg(not(windows))]
fn editor_candidates(editor: Editor) -> [&'static str; 1] {
    [editor.command_name()]
}

#[cfg(windows)]
fn editor_candidates(editor: Editor) -> [String; 5] {
    let name = editor.command_name();
    [
        format!("{name}.exe"),
        format!("{name}.cmd"),
        format!("{name}.bat"),
        format!("{name}.com"),
        name.to_owned(),
    ]
}

#[cfg(not(windows))]
fn run_install(editor_cli: &Path, vsix: &Path) -> std::io::Result<Output> {
    Command::new(editor_cli)
        .arg("--install-extension")
        .arg(vsix)
        .output()
}

#[cfg(windows)]
fn run_install(editor_cli: &Path, vsix: &Path) -> std::io::Result<Output> {
    let is_script = editor_cli
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        });
    if is_script {
        Command::new("cmd.exe")
            .args(["/D", "/S", "/C"])
            .arg(editor_cli)
            .arg("--install-extension")
            .arg(vsix)
            .output()
    } else {
        Command::new(editor_cli)
            .arg("--install-extension")
            .arg(vsix)
            .output()
    }
}

fn editor_failure(editor: Editor, output: &Output) -> CrivError {
    let status = output.status.code().map_or_else(
        || "without an exit code".to_owned(),
        |code| code.to_string(),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut message = format!(
        "editor install failed: {} --install-extension exited {status}",
        editor.command_name()
    );
    if !stdout.trim().is_empty() {
        message.push_str("\neditor stdout:\n");
        message.push_str(stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        message.push_str("\neditor stderr:\n");
        message.push_str(stderr.trim_end());
    }
    CrivError::new(message)
}

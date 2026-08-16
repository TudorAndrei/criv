use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-adapter",
    about = "Prepare an immutable criv tag with its test-only discovery probe adapter"
)]
struct Args {
    /// Source criv repository.
    #[arg(long, default_value = ".")]
    repository_root: PathBuf,
    /// Immutable tag or commit to export.
    #[arg(long, default_value = "v0.9.0")]
    revision: String,
    /// New output directory. It must not exist.
    #[arg(long)]
    output: PathBuf,
    /// Adapter patch relative to repository-root.
    #[arg(
        long,
        default_value = "scripts/performance/adapters/v0.9.0-discovery-probe.patch"
    )]
    patch: PathBuf,
    /// Shared probe source relative to repository-root.
    #[arg(long, default_value = "scripts/performance/discovery_probe.rs")]
    probe_source: PathBuf,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    revision: String,
    commit: String,
    patch_digest: String,
    probe_digest: String,
    output: String,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-adapter: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let repository_root = fs::canonicalize(&args.repository_root).map_err(display_error)?;
    let output = resolve_new_output(&args.output)?;
    let patch = resolve_input(&repository_root, &args.patch)?;
    let probe_source = resolve_input(&repository_root, &args.probe_source)?;
    let commit = command_text(
        &repository_root,
        "git",
        &["rev-parse", &format!("{}^{{commit}}", args.revision)],
    )?;
    let archive = tempfile::NamedTempFile::new().map_err(display_error)?;
    let archive_path = archive.path().to_path_buf();
    drop(archive);
    let archive_status = Command::new("git")
        .args(["archive", "--format=tar", "--output"])
        .arg(&archive_path)
        .arg(&commit)
        .current_dir(&repository_root)
        .status()
        .map_err(display_error)?;
    if !archive_status.success() {
        return Err(format!("git archive failed for {commit}"));
    }

    fs::create_dir(&output).map_err(display_error)?;
    if let Err(error) = prepare_export(&output, &archive_path, &patch, &probe_source) {
        cleanup_output(&output);
        return Err(error);
    }
    let receipt = Receipt {
        schema: "criv.discovery-adapter.v1",
        revision: args.revision,
        commit,
        patch_digest: file_digest(&patch)?,
        probe_digest: file_digest(&probe_source)?,
        output: output.display().to_string(),
    };
    fs::write(
        output.join("discovery-adapter.json"),
        serde_json::to_vec_pretty(&receipt).map_err(display_error)?,
    )
    .map_err(display_error)?;
    println!("{}", output.display());
    Ok(())
}

fn prepare_export(
    output: &Path,
    archive: &Path,
    patch: &Path,
    probe_source: &Path,
) -> Result<(), String> {
    let extract = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(output)
        .status()
        .map_err(display_error)?;
    if !extract.success() {
        return Err("failed to extract source archive".into());
    }
    let destination = output.join("scripts/performance/discovery_probe.rs");
    let parent = destination
        .parent()
        .ok_or_else(|| "probe destination has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(display_error)?;
    fs::copy(probe_source, &destination).map_err(display_error)?;
    let ceiling = output
        .parent()
        .ok_or_else(|| "adapter output has no parent".to_string())?;
    let check = Command::new("git")
        .args(["apply", "--check"])
        .arg(patch)
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .current_dir(output)
        .status()
        .map_err(display_error)?;
    if !check.success() {
        return Err("discovery adapter patch does not apply to the exported revision".into());
    }
    let apply = Command::new("git")
        .arg("apply")
        .arg(patch)
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .current_dir(output)
        .status()
        .map_err(display_error)?;
    if !apply.success() {
        return Err("failed to apply discovery adapter patch".into());
    }
    let lib = fs::read_to_string(output.join("src/lib.rs")).map_err(display_error)?;
    let check = fs::read_to_string(output.join("src/check.rs")).map_err(display_error)?;
    if !lib.contains("mod discovery_probe;") || !check.contains("fn discovery_probe_markdown_files")
    {
        return Err(
            "discovery adapter patch reported success but the probe hooks are absent".into(),
        );
    }
    Ok(())
}

fn resolve_new_output(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return Err(format!("output already exists: {}", path.display()));
    }
    let name = path
        .file_name()
        .ok_or_else(|| "output must name a new directory".to_string())?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(display_error)?;
    Ok(parent.join(name))
}

fn resolve_input(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    fs::canonicalize(&path)
        .map_err(|error| format!("failed to resolve {}: {error}", path.display()))
}

fn cleanup_output(output: &Path) {
    if let Err(error) = fs::remove_dir_all(output) {
        eprintln!(
            "criv-discovery-adapter: failed to remove incomplete output {}: {error}",
            output.display()
        );
    }
}

fn command_text(root: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(display_error)?;
    if !output.status.success() {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(display_error)
}

fn file_digest(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(display_error)
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_must_not_exist() {
        let root = tempfile::TempDir::new().unwrap();
        let output = root.path().join("existing");
        fs::create_dir(&output).unwrap();
        assert!(
            resolve_new_output(&output)
                .unwrap_err()
                .contains("already exists")
        );
    }

    #[test]
    fn relative_inputs_resolve_from_repository() {
        let root = tempfile::TempDir::new().unwrap();
        fs::write(root.path().join("input"), "value").unwrap();
        assert_eq!(
            resolve_input(root.path(), Path::new("input")).unwrap(),
            fs::canonicalize(root.path().join("input")).unwrap()
        );
    }
}

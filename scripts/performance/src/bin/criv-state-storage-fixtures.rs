#[path = "../generate.rs"]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "this binary shares the fixture generator but not every workload"
    )
)]
mod generate;
#[path = "../manifest.rs"]
#[expect(
    dead_code,
    reason = "this binary shares the manifest model but not every field reader"
)]
mod manifest;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use generate::{append_source_revision, generate};
use manifest::LoadedManifest;
use serde::Serialize;
use usage::Cli;

const OUTPUT_SCHEMA: &str = "criv.state-storage-fixtures.v1";

/// Generate observed-shape State revisions for store candidate measurements
#[derive(Debug, Cli)]
#[usage(bin = "criv-state-storage-fixtures", unknown_flags = "error")]
struct Args {
    #[usage(long)]
    binary: PathBuf,
    #[usage(long)]
    manifest: PathBuf,
    #[usage(long)]
    output: PathBuf,
    #[usage(long, default = "20")]
    snapshots: usize,
}

#[derive(Debug, Serialize)]
struct Output {
    schema: &'static str,
    workload: String,
    workload_manifest: String,
    workload_digest: String,
    generated_digest: String,
    snapshots: Vec<Snapshot>,
}

#[derive(Debug, Serialize)]
struct Snapshot {
    revision: usize,
    path: String,
    bytes: usize,
    digest: String,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-state-storage-fixtures: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    if args.snapshots < 2 {
        return Err("--snapshots must be at least 2".into());
    }
    ensure_empty_output(&args.output)?;
    let binary = fs::canonicalize(&args.binary)
        .map_err(|error| format!("failed to resolve {}: {error}", args.binary.display()))?;
    let loaded = LoadedManifest::load(&args.manifest)?;
    let vault = tempfile::tempdir().map_err(display_error)?;
    let generated = generate(vault.path(), &loaded.manifest)?;
    let mut snapshots = Vec::with_capacity(args.snapshots);

    for revision in 0..args.snapshots {
        if revision > 0 {
            append_source_revision(vault.path(), &generated, revision + 1)?;
        }
        publish_state(&binary, vault.path(), revision)?;
        let source = vault.path().join(".criv/state.json");
        let name = format!("{revision:03}.json");
        let destination = args.output.join(&name);
        let bytes = fs::read(&source)
            .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
        fs::write(&destination, &bytes).map_err(display_error)?;
        snapshots.push(Snapshot {
            revision,
            path: name,
            bytes: bytes.len(),
            digest: blake3::hash(&bytes).to_hex().to_string(),
        });
    }

    let output = Output {
        schema: OUTPUT_SCHEMA,
        workload: loaded.manifest.id,
        workload_manifest: loaded.path.display().to_string(),
        workload_digest: loaded.digest,
        generated_digest: generated.digest,
        snapshots,
    };
    fs::write(
        args.output.join("manifest.json"),
        serde_json::to_vec_pretty(&output)
            .map_err(|error| format!("failed to encode fixture manifest: {error}"))?,
    )
    .map_err(display_error)?;
    Ok(())
}

fn ensure_empty_output(path: &Path) -> Result<(), String> {
    if path.exists() && fs::read_dir(path).map_err(display_error)?.next().is_some() {
        return Err(format!("output directory is not empty: {}", path.display()));
    }
    fs::create_dir_all(path).map_err(display_error)
}

fn publish_state(binary: &Path, vault: &Path, revision: usize) -> Result<(), String> {
    let output = Command::new(binary)
        .args(["watch", "--once"])
        .current_dir(vault)
        .env("CRIV_PERF_CASE", "state_store_candidate_fixture")
        .env("CRIV_PERF_SAMPLE_ID", revision.to_string())
        .env_remove("CRIV_BASE_REF")
        .env_remove("GITHUB_BASE_REF")
        .output()
        .map_err(|error| format!("failed to start criv: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "State publication failed for revision {revision}:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

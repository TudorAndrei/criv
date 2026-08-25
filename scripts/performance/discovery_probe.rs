use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{CrivError, Result};

const PROBE_SCHEMA: &str = "criv.discovery-probe.v1";
const PROBE_PREFIX: &str = "criv-discovery-probe-v1 ";
const ROOT_ENV: &str = "CRIV_DISCOVERY_PROBE_ROOT";
const DUMP_ENV: &str = "CRIV_DISCOVERY_PROBE_DUMP";

#[derive(Debug, Serialize)]
struct ProbeGroup {
    name: &'static str,
    selected_files: usize,
    selected_bytes: u64,
    path_digest: String,
}

#[derive(Debug, Serialize)]
struct ProbeOutput {
    schema: &'static str,
    profile: &'static str,
    selected_files: usize,
    selected_bytes: u64,
    path_digest: String,
    groups: Vec<ProbeGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<BTreeMap<&'static str, Vec<String>>>,
}

#[test]
#[ignore = "run through criv-discovery-baseline"]
fn source() {
    emit(run_source(&probe_root().unwrap()).unwrap());
}

#[test]
#[ignore = "run through criv-discovery-baseline"]
fn source_candidates() {
    emit(run_source_candidates(&probe_root().unwrap()).unwrap());
}

#[test]
#[ignore = "run through criv-discovery-baseline"]
fn vault() {
    emit(run_vault(&probe_root().unwrap()).unwrap());
}

#[test]
#[ignore = "run through criv-discovery-baseline"]
fn markdown() {
    emit(run_markdown(&probe_root().unwrap()).unwrap());
}

fn run_source(root: &Path) -> Result<ProbeOutput> {
    let paths = crate::discovery_probe_source_files(root)?;
    output(root, "source", vec![("source", paths)])
}

fn run_source_candidates(root: &Path) -> Result<ProbeOutput> {
    let paths = crate::discovery_probe_source_candidates(root)?;
    output(
        root,
        "source_candidates",
        vec![("source_candidates", paths)],
    )
}

fn run_vault(root: &Path) -> Result<ProbeOutput> {
    let (markdown, c4) = crate::discovery_probe_vault_files(root)?;
    output(root, "vault", vec![("markdown", markdown), ("c4", c4)])
}

fn run_markdown(root: &Path) -> Result<ProbeOutput> {
    let paths = crate::check::discovery_probe_markdown_files(root)?;
    output(root, "markdown", vec![("markdown", paths)])
}

fn output(
    root: &Path,
    profile: &'static str,
    groups: Vec<(&'static str, Vec<String>)>,
) -> Result<ProbeOutput> {
    let mut group_results = Vec::with_capacity(groups.len());
    let mut all_paths = Vec::new();
    let mut dumped_paths = BTreeMap::new();
    let dump = std::env::var_os(DUMP_ENV).is_some();

    for (name, paths) in groups {
        let selected_bytes = selected_bytes(root, &paths)?;
        let path_digest = path_digest(name, &paths);
        all_paths.extend(paths.iter().map(|path| format!("{name}\0{path}")));
        group_results.push(ProbeGroup {
            name,
            selected_files: paths.len(),
            selected_bytes,
            path_digest,
        });
        if dump {
            dumped_paths.insert(name, paths);
        }
    }

    Ok(ProbeOutput {
        schema: PROBE_SCHEMA,
        profile,
        selected_files: group_results.iter().map(|group| group.selected_files).sum(),
        selected_bytes: group_results.iter().map(|group| group.selected_bytes).sum(),
        path_digest: path_digest(profile, &all_paths),
        groups: group_results,
        paths: dump.then_some(dumped_paths),
    })
}

fn selected_bytes(root: &Path, paths: &[String]) -> Result<u64> {
    paths.iter().try_fold(0_u64, |total, path| {
        let bytes = fs::metadata(root.join(path)).map_err(|error| {
            CrivError::new(format!(
                "failed to read selected file metadata for {path}: {error}"
            ))
        })?;
        Ok(total.saturating_add(bytes.len()))
    })
}

fn path_digest(domain: &str, paths: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(PROBE_SCHEMA.as_bytes());
    hasher.update(
        &u64::try_from(domain.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    hasher.update(domain.as_bytes());
    for path in paths {
        hasher.update(&u64::try_from(path.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(path.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn probe_root() -> Result<PathBuf> {
    let root = std::env::var_os(ROOT_ENV)
        .ok_or_else(|| CrivError::new(format!("{ROOT_ENV} must name the measured repository")))?;
    fs::canonicalize(root).map_err(CrivError::from)
}

fn emit(output: ProbeOutput) {
    println!(
        "{PROBE_PREFIX}{}",
        serde_json::to_string(&output).expect("serialize discovery probe output")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_identity_is_order_sensitive() {
        let first = path_digest("source", &["src/a.rs".into(), "src/b.rs".into()]);
        let second = path_digest("source", &["src/b.rs".into(), "src/a.rs".into()]);
        assert_ne!(first, second);
    }

    #[test]
    fn path_identity_is_domain_separated() {
        let paths = ["docs/a.md".into()];
        assert_ne!(path_digest("markdown", &paths), path_digest("c4", &paths));
    }

    #[test]
    fn probe_output_keeps_vault_group_identities() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/a.md"), "# A\n").unwrap();
        fs::write(root.path().join("docs/model.c4"), "model {}\n").unwrap();

        let output = output(
            root.path(),
            "vault",
            vec![
                ("markdown", vec!["docs/a.md".into()]),
                ("c4", vec!["docs/model.c4".into()]),
            ],
        )
        .unwrap();

        assert_eq!(output.selected_files, 2);
        assert_eq!(output.groups.len(), 2);
        assert_ne!(output.groups[0].path_digest, output.groups[1].path_digest);
    }
}

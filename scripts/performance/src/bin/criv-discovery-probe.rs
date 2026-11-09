use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use usage_rs::Cli;

const PROBE_SCHEMA: &str = "criv.discovery-probe.v1";
const PROBE_PREFIX: &str = "criv-discovery-probe-v1 ";

/// Print the files criv selects for one discovery profile
#[derive(Debug, Cli)]
#[usage(bin = "criv-discovery-probe", unknown_flags = "error")]
struct Args {
    /// Discovery profile: source, source_candidates, or vault.
    #[usage(long)]
    profile: String,
    /// Repository tree whose file discovery is measured.
    #[usage(long)]
    root: PathBuf,
    /// Include the complete selected path lists.
    #[usage(long)]
    dump_paths: bool,
}

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

fn main() {
    if let Err(error) = run(&Args::parse()) {
        eprintln!("criv-discovery-probe: {error}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let root = fs::canonicalize(&args.root).map_err(display_error)?;
    let (profile, group, paths) = match args.profile.as_str() {
        "source" => ("source", "source", criv::selection::source_files(&root)),
        "source_candidates" => (
            "source_candidates",
            "source_candidates",
            criv::selection::source_candidates(&root),
        ),
        "vault" => ("vault", "markdown", criv::selection::vault_markdown(&root)),
        other => return Err(format!("unknown discovery profile {other}")),
    };
    let paths = paths.map_err(display_error)?;
    let output = output(&root, profile, vec![(group, paths)], args.dump_paths)?;
    println!(
        "{PROBE_PREFIX}{}",
        serde_json::to_string(&output).map_err(display_error)?
    );
    Ok(())
}

fn output(
    root: &Path,
    profile: &'static str,
    groups: Vec<(&'static str, Vec<String>)>,
    dump: bool,
) -> Result<ProbeOutput, String> {
    let mut group_results = Vec::with_capacity(groups.len());
    let mut all_paths = Vec::new();
    let mut dumped_paths = BTreeMap::new();

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

fn selected_bytes(root: &Path, paths: &[String]) -> Result<u64, String> {
    paths.iter().try_fold(0_u64, |total, path| {
        let bytes = fs::metadata(root.join(path)).map_err(|error| {
            format!("failed to read selected file metadata for {path}: {error}")
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

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
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
    fn probe_output_keeps_group_identities() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/a.md"), "# A\n").unwrap();
        fs::write(root.path().join("docs/b.md"), "# B\n").unwrap();

        let output = output(
            root.path(),
            "vault",
            vec![
                ("markdown", vec!["docs/a.md".into()]),
                ("other", vec!["docs/b.md".into()]),
            ],
            false,
        )
        .unwrap();

        assert_eq!(output.selected_files, 2);
        assert_eq!(output.groups.len(), 2);
        assert_ne!(output.groups[0].path_digest, output.groups[1].path_digest);
    }

    #[test]
    fn vault_profile_selects_markdown_through_criv() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/a.md"), "# A\n").unwrap();

        assert_eq!(
            criv::selection::vault_markdown(root.path()).unwrap(),
            vec!["docs/a.md".to_string()]
        );
    }
}

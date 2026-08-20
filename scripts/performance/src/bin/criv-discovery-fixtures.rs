use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use serde::{Deserialize, Serialize};

const MANIFEST_SCHEMA: &str = "criv.discovery-workload.v1";
const FIXTURE_COMMIT_DATE: &str = "2000-01-01T00:00:00Z";

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-fixtures",
    about = "Generate one deterministic file-discovery scaling workload"
)]
struct Args {
    /// Discovery workload manifest.
    #[arg(long)]
    manifest: PathBuf,
    /// New generated repository directory. It must not exist.
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Profile {
    Source,
    Vault,
    Markdown,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    id: String,
    profile: Profile,
    seed: u64,
    generated_entries: usize,
    generated_directories: usize,
    maximum_depth: usize,
    fanout: usize,
    minimum_file_bytes: usize,
    expected: Expected,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    source_files: usize,
    vault_markdown_files: usize,
    vault_c4_files: usize,
    markdown_files: usize,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Receipt<'a> {
    schema: &'static str,
    manifest: &'a Manifest,
    manifest_digest: String,
    generated_files: usize,
    generated_directories: usize,
    observed_maximum_depth: usize,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-fixtures: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let manifest_bytes = fs::read(&args.manifest).map_err(display_error)?;
    let manifest: Manifest =
        toml::from_str(std::str::from_utf8(&manifest_bytes).map_err(display_error)?)
            .map_err(display_error)?;
    validate_manifest(&manifest)?;
    if args.output.exists() {
        return Err(format!("output already exists: {}", args.output.display()));
    }
    let parent = args
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "output parent is not a directory: {}",
            parent.display()
        ));
    }

    fs::create_dir(&args.output).map_err(display_error)?;
    let generated = match generate(&manifest, &args.output) {
        Ok(generated) => generated,
        Err(error) => {
            cleanup_output(&args.output);
            return Err(error);
        }
    };
    let receipt = Receipt {
        schema: "criv.discovery-fixture-receipt.v1",
        manifest: &manifest,
        manifest_digest: bytes_digest(&manifest_bytes),
        generated_files: generated.files,
        generated_directories: generated.directories,
        observed_maximum_depth: generated.maximum_depth,
    };
    write_json(args.output.join("discovery-fixture.json"), &receipt)?;
    if let Err(error) = initialize_git(&args.output) {
        cleanup_output(&args.output);
        return Err(error);
    }
    println!("{}", args.output.display());
    Ok(())
}

#[derive(Debug, Default)]
struct Generated {
    files: usize,
    directories: usize,
    maximum_depth: usize,
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(format!(
            "unsupported discovery workload schema {}",
            manifest.schema
        ));
    }
    if manifest.id.trim().is_empty() {
        return Err("manifest id must not be empty".into());
    }
    if manifest.generated_entries == 0 {
        return Err("generated-entries must be positive".into());
    }
    if manifest.generated_directories >= manifest.generated_entries {
        return Err("generated-directories must be smaller than generated-entries".into());
    }
    if manifest.maximum_depth == 0 || manifest.fanout == 0 {
        return Err("maximum-depth and fanout must be positive".into());
    }
    let files = manifest.generated_entries - manifest.generated_directories;
    let expected_files = match manifest.profile {
        Profile::Source => manifest.expected.source_files,
        Profile::Vault => manifest.expected.vault_markdown_files + manifest.expected.vault_c4_files,
        Profile::Markdown => manifest.expected.markdown_files,
    };
    if expected_files != files {
        return Err(format!(
            "expected selected file count {expected_files} does not match generated file count {files}"
        ));
    }
    if !manifest.expected.errors.is_empty() {
        return Err("scaling manifests must not declare expected errors".into());
    }
    Ok(())
}

fn generate(manifest: &Manifest, root: &Path) -> Result<Generated, String> {
    fs::write(
        root.join("criv.toml"),
        "[vault]\ndocs = \"docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"src\"]\nexclude = [\"**/target/**\", \"**/node_modules/**\"]\n\n[index]\nsource = true\n",
    )
    .map_err(display_error)?;
    fs::write(
        root.join(".rumdl.toml"),
        "[global]\ndisable = [\"MD013\", \"MD025\", \"MD041\", \"MD075\"]\nexclude = [\".criv/**\"]\n",
    )
    .map_err(display_error)?;
    fs::write(root.join(".gitignore"), ".criv/\n").map_err(display_error)?;
    fs::create_dir(root.join("src")).map_err(display_error)?;
    fs::create_dir(root.join("docs")).map_err(display_error)?;
    fs::create_dir(root.join("content")).map_err(display_error)?;

    let base = match manifest.profile {
        Profile::Source => root.join("src/generated"),
        Profile::Vault => root.join("docs/generated"),
        Profile::Markdown => root.join("content/generated"),
    };
    fs::create_dir(&base).map_err(display_error)?;
    let mut directories = vec![base.clone()];
    let mut observed_depth = 0;
    for index in 0..manifest.generated_directories {
        let parent_index = index / manifest.fanout;
        let parent = directories.get(parent_index).ok_or_else(|| {
            format!(
                "fanout {} cannot place generated directory {index}",
                manifest.fanout
            )
        })?;
        let directory = parent.join(format!("d{index:08}"));
        fs::create_dir(&directory).map_err(display_error)?;
        let depth = directory
            .strip_prefix(&base)
            .map_err(display_error)?
            .components()
            .count();
        if depth > manifest.maximum_depth {
            return Err(format!(
                "generated depth {depth} exceeds maximum-depth {}",
                manifest.maximum_depth
            ));
        }
        observed_depth = observed_depth.max(depth);
        directories.push(directory);
    }

    let files = manifest.generated_entries - manifest.generated_directories;
    for index in 0..files {
        let directory = &directories[index % directories.len()];
        let (extension, prefix) = match manifest.profile {
            Profile::Source => ("rs", format!("pub fn generated_{index:08}() {{}}\n//")),
            Profile::Vault => (
                "md",
                format!(
                    "---\nid: generated-{index:08}\nkind: doc\ntitle: Generated {index}\n---\n\n# Generated {index}\n"
                ),
            ),
            Profile::Markdown => ("md", format!("# Generated {index}\n")),
        };
        let path = directory.join(format!("f{index:08}.{extension}"));
        write_padded(
            &path,
            &prefix,
            manifest.minimum_file_bytes,
            manifest.seed,
            index,
        )?;
    }

    Ok(Generated {
        files,
        directories: manifest.generated_directories,
        maximum_depth: observed_depth,
    })
}

fn write_padded(
    path: &Path,
    prefix: &str,
    minimum_bytes: usize,
    seed: u64,
    index: usize,
) -> Result<(), String> {
    let mut file = BufWriter::new(File::create(path).map_err(display_error)?);
    file.write_all(prefix.as_bytes()).map_err(display_error)?;
    let mut written = prefix.len();
    let pattern = format!(" generated-{seed:016x}-{index:08} ");
    while written < minimum_bytes {
        let remaining = minimum_bytes - written;
        let bytes = pattern.as_bytes();
        let count = remaining.min(bytes.len());
        file.write_all(&bytes[..count]).map_err(display_error)?;
        written += count;
    }
    file.write_all(b"\n").map_err(display_error)
}

fn initialize_git(root: &Path) -> Result<(), String> {
    for args in [
        &["init", "--quiet"][..],
        &["config", "user.email", "performance@criv.invalid"][..],
        &["config", "user.name", "criv performance"][..],
        &["config", "gc.auto", "0"][..],
        &["config", "gc.autoDetach", "false"][..],
        &["config", "maintenance.auto", "false"][..],
        &["config", "maintenance.autoDetach", "false"][..],
        &["add", "--all"][..],
        &["commit", "--quiet", "-m", "fixture root"][..],
        &["gc", "--quiet"][..],
    ] {
        let output = Command::new("git")
            .args(args)
            .env("GIT_AUTHOR_DATE", FIXTURE_COMMIT_DATE)
            .env("GIT_COMMITTER_DATE", FIXTURE_COMMIT_DATE)
            .current_dir(root)
            .output()
            .map_err(display_error)?;
        if !output.status.success() {
            return Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    Ok(())
}

fn cleanup_output(output: &Path) {
    if let Err(error) = fs::remove_dir_all(output) {
        eprintln!(
            "criv-discovery-fixtures: failed to remove incomplete output {}: {error}",
            output.display()
        );
    }
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), String> {
    let mut output = BufWriter::new(File::create(path).map_err(display_error)?);
    serde_json::to_writer_pretty(&mut output, value).map_err(display_error)?;
    output.write_all(b"\n").map_err(display_error)
}

fn bytes_digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            schema: MANIFEST_SCHEMA.into(),
            id: "source-test".into(),
            profile: Profile::Source,
            seed: 7,
            generated_entries: 20,
            generated_directories: 4,
            maximum_depth: 3,
            fanout: 4,
            minimum_file_bytes: 64,
            expected: Expected {
                source_files: 16,
                vault_markdown_files: 0,
                vault_c4_files: 0,
                markdown_files: 0,
                errors: vec![],
            },
        }
    }

    #[test]
    fn selected_count_must_match_generated_files() {
        let mut manifest = manifest();
        manifest.expected.source_files = 15;
        assert!(
            validate_manifest(&manifest)
                .unwrap_err()
                .contains("does not match generated file count")
        );
    }

    #[test]
    fn generator_creates_exact_requested_shape() {
        let root = tempfile::TempDir::new().unwrap();
        let generated = generate(&manifest(), root.path()).unwrap();
        assert_eq!(generated.files, 16);
        assert_eq!(generated.directories, 4);
        assert!(generated.maximum_depth <= 3);
        assert_eq!(
            fs::read(root.path().join("src/generated/f00000000.rs"))
                .unwrap()
                .len(),
            65
        );
    }

    #[test]
    fn generated_git_commit_is_repeatable() {
        let left = tempfile::TempDir::new().unwrap();
        let right = tempfile::TempDir::new().unwrap();
        fs::write(left.path().join("input.txt"), "same\n").unwrap();
        fs::write(right.path().join("input.txt"), "same\n").unwrap();

        initialize_git(left.path()).unwrap();
        initialize_git(right.path()).unwrap();

        let revision = |root: &Path| {
            let output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(root)
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        assert_eq!(revision(left.path()), revision(right.path()));
    }

    #[test]
    fn generated_git_repository_is_stable_before_returning() {
        let root = tempfile::TempDir::new().unwrap();
        fs::write(root.path().join("input.txt"), "same\n").unwrap();

        initialize_git(root.path()).unwrap();

        let config = Command::new("git")
            .args(["config", "--get", "gc.auto"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(config.status.success());
        assert_eq!(String::from_utf8(config.stdout).unwrap().trim(), "0");

        let counts = Command::new("git")
            .args(["count-objects", "-v"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(counts.status.success());
        let counts = String::from_utf8(counts.stdout).unwrap();
        assert!(counts.lines().any(|line| line == "count: 0"));
        assert!(counts.lines().any(|line| {
            line.strip_prefix("in-pack: ")
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|value| value > 0)
        }));
    }

    #[test]
    fn schema_is_required() {
        let mut manifest = manifest();
        manifest.schema = "unknown".into();
        assert_eq!(
            validate_manifest(&manifest).unwrap_err(),
            "unsupported discovery workload schema unknown"
        );
    }
}

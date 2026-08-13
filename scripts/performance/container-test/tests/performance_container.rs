//! Explicit Docker/Testcontainers performance-environment coverage.
//!
//! The container is only the execution environment. The vault-shaped workloads
//! remain generated from the same checked-in manifests used by host runs.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use testcontainers::core::{BuildImageOptions, WaitFor};
use testcontainers::runners::{SyncBuilder, SyncRunner};
use testcontainers::{GenericBuildableImage, ImageExt};

const RUST_IMAGE: &str =
    "rust:1.97.1-bookworm@sha256:77fac8b98f9f46062bb680b6d25d5bcaabfc400143952ebc572e924bcbedc3fa";
const READY_MESSAGE: &str = "criv-performance-container-ok";

#[test]
#[ignore = "requires a Docker-compatible runtime and builds a release image"]
fn performance_harness_runs_in_pinned_container() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("resolve repository root");
    let context = stage_build_context(&repository);
    let tag = context_digest(context.path());
    let dockerfile = format!(
        r#"FROM {RUST_IMAGE}
WORKDIR /workspace
COPY . .
RUN cargo build --release --locked --package criv --package criv-perf-harness
CMD ["bash", "-c", "target/release/criv-perf-harness --repository-root /workspace --binary /workspace/target/release/criv --profile release --samples 1 --allow-low-samples --case check --manifest fixtures/performance/barrs-small.toml --results-root /tmp/criv-performance-results && test -n \"$(find /tmp/criv-performance-results -name summary.json -print -quit)\" && echo {READY_MESSAGE}"]
"#
    );

    let image = GenericBuildableImage::new("criv-performance-environment", &tag)
        .with_dockerfile_string(dockerfile)
        .with_file(context.path(), ".")
        .build_image_with(BuildImageOptions::new().with_skip_if_exists(true))
        .expect("build the pinned criv performance image");

    let container = image
        .with_wait_for(WaitFor::message_on_stdout(READY_MESSAGE))
        .with_startup_timeout(Duration::from_secs(20 * 60))
        .start()
        .expect("run the performance harness in Docker");
    let stdout = String::from_utf8(container.stdout_to_vec().expect("read container stdout"))
        .expect("container stdout is UTF-8");
    assert!(stdout.contains(READY_MESSAGE), "container stdout: {stdout}");
}

fn stage_build_context(repository: &Path) -> tempfile::TempDir {
    let context = tempfile::TempDir::new().expect("create Docker build context");
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(repository)
        .output()
        .expect("list repository files");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for raw_path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = PathBuf::from(OsStr::new(
            std::str::from_utf8(raw_path).expect("git path is UTF-8"),
        ));
        let source = repository.join(&relative);
        let destination = context.path().join(&relative);
        if fs::symlink_metadata(&source)
            .expect("inspect repository input")
            .file_type()
            .is_symlink()
        {
            // Generated-skill convenience links are not release build inputs,
            // and dereferencing a directory link would duplicate the context.
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).expect("create staged parent directory");
        }
        fs::copy(&source, &destination).unwrap_or_else(|error| {
            panic!("copy {} into Docker context: {error}", relative.display())
        });
    }
    context
}

fn context_digest(root: &Path) -> String {
    fn visit(root: &Path, current: &Path, hasher: &mut blake3::Hasher) {
        let mut entries = fs::read_dir(current)
            .expect("read Docker build context")
            .map(|entry| entry.expect("read Docker build context entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, hasher);
            } else {
                let relative = path.strip_prefix(root).expect("path is below context root");
                hasher.update(relative.to_string_lossy().as_bytes());
                hasher.update(&fs::read(path).expect("read staged Docker input"));
            }
        }
    }

    let mut hasher = blake3::Hasher::new();
    visit(root, root, &mut hasher);
    hasher.finalize().to_hex()[..16].to_owned()
}

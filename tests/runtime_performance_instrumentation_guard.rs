#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::pedantic,
    clippy::unwrap_used
)]

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn production_sources_do_not_contain_performance_instrumentation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    collect_rust_sources(&root.join("src"), &mut sources);

    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "CRIV_PERF_",
            "performance-measurement",
            "crate::measurement",
            "mod measurement",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains forbidden performance instrumentation marker {forbidden}",
                path.strip_prefix(root).unwrap().display()
            );
        }
    }
}

fn collect_rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, sources);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            sources.push(path);
        }
    }
}

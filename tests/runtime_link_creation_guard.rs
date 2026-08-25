#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::pedantic,
    clippy::unwrap_used
)]

use std::fs;
use std::path::{Path, PathBuf};

const LINK_HELPER: &str = "repository/filesystem.rs";

#[test]
fn production_modules_create_links_only_through_the_confined_helper() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = collect_rust_sources(&root.join("src"));
    assert!(
        sources.len() > 20,
        "the guard must walk the whole source tree, found {} files",
        sources.len()
    );

    let mut helper_seen = false;
    for path in sources {
        let relative = path.strip_prefix(root.join("src")).unwrap_or(&path);
        let relative = relative.to_string_lossy().replace('\\', "/");
        let is_helper = relative == LINK_HELPER;
        helper_seen |= is_helper;
        if is_helper || relative.ends_with("tests.rs") {
            continue;
        }

        let source = fs::read_to_string(&path).unwrap();
        let production = source.split("#[cfg(test)]").next().unwrap();
        for call in ["std::os::unix::fs::symlink(", "junction::create("] {
            assert!(
                !production.contains(call),
                "{} creates a link outside RepositoryWriteScope::link_dir",
                path.display()
            );
        }
    }

    assert!(
        helper_seen,
        "the confined helper moved; update LINK_HELPER and ADR-0137's governs list"
    );
}

fn collect_rust_sources(directory: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return sources;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    sources.sort();
    sources
}

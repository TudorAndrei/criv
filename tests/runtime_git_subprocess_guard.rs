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
fn production_git_modules_do_not_launch_the_git_executable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = collect_rust_sources(&root.join("src"));
    assert!(
        sources.len() > 20,
        "the guard must walk the whole source tree, found {} files",
        sources.len()
    );

    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(
            !production.contains("Command::new(\"git\")"),
            "{} launches the git executable outside test-only code",
            path.display()
        );
    }
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

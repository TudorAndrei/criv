use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::WorkloadManifest;

#[derive(Debug)]
pub struct GeneratedWorkload {
    pub source_paths: Vec<PathBuf>,
    pub digest: String,
}

pub fn generate(root: &Path, manifest: &WorkloadManifest) -> Result<GeneratedWorkload, String> {
    if root.exists() && fs::read_dir(root).map_err(display_error)?.next().is_some() {
        return Err(format!(
            "generated workload destination is not empty: {}",
            root.display()
        ));
    }
    fs::create_dir_all(root.join("src")).map_err(display_error)?;
    fs::create_dir_all(root.join("docs/adr")).map_err(display_error)?;
    fs::create_dir_all(root.join("docs/architecture")).map_err(display_error)?;

    let source_paths = source_paths(manifest);
    let mut contents = source_contents(manifest, &source_paths)?;
    pad_source_bytes(manifest, &source_paths, &mut contents)?;
    for path in &source_paths {
        let contents = contents
            .remove(path)
            .ok_or_else(|| format!("missing generated contents for {}", path.display()))?;
        fs::write(root.join(path), contents).map_err(display_error)?;
    }

    write_config(root)?;
    write_notes(root, manifest, &source_paths)?;
    write_c4_artifacts(root, manifest.c4_artifacts)?;
    fs::write(root.join(".gitignore"), ".criv/\n").map_err(display_error)?;
    initialize_git(root)?;

    Ok(GeneratedWorkload {
        source_paths,
        digest: tree_digest(root)?,
    })
}

pub fn mutate_sources(
    root: &Path,
    generated: &GeneratedWorkload,
    count: usize,
) -> Result<(), String> {
    let mut paths = generated.source_paths.iter().collect::<Vec<_>>();
    paths.sort_by_key(|path| mutation_priority(path));
    for path in paths.into_iter().take(count) {
        let path = root.join(path);
        let mut contents = fs::read(&path).map_err(display_error)?;
        if contents.is_empty() {
            return Err(format!("cannot mutate empty source {}", path.display()));
        }
        let index = contents.len() - 1;
        contents[index] = if contents[index] == b' ' { b'\n' } else { b' ' };
        fs::write(path, contents).map_err(display_error)?;
    }
    Ok(())
}

fn mutation_priority(path: &Path) -> u8 {
    match path.extension().and_then(|value| value.to_str()) {
        Some("rs") => 0,
        Some("ts") => 1,
        Some("mjs") => 2,
        _ => 3,
    }
}

fn source_paths(manifest: &WorkloadManifest) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(manifest.source_files);
    let mut index = 0usize;
    for (extension, count) in &manifest.extensions {
        for _ in 0..*count {
            let name = match extension.as_str() {
                "none" => format!("source-{index:04}"),
                other => format!("source-{index:04}.{other}"),
            };
            paths.push(Path::new("src").join(name));
            index += 1;
        }
    }
    paths
}

fn source_contents(
    manifest: &WorkloadManifest,
    paths: &[PathBuf],
) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut contents = paths
        .iter()
        .cloned()
        .map(|path| (path, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let code_paths = paths
        .iter()
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("rs" | "ts" | "mjs")
            )
        })
        .collect::<Vec<_>>();
    if code_paths.is_empty() {
        return Err("workload needs a supported code file for symbols".into());
    }

    for symbol in 0..manifest.symbols {
        let path = code_paths[symbol % code_paths.len()];
        let extension = path.extension().and_then(|value| value.to_str()).unwrap();
        let line = match extension {
            "rs" => format!("pub fn symbol_{symbol:06}() -> usize {{ {symbol} }}\n"),
            "ts" => {
                format!("export function symbol_{symbol:06}(): number {{ return {symbol}; }}\n")
            }
            "mjs" => format!("export function symbol_{symbol:06}() {{ return {symbol}; }}\n"),
            _ => unreachable!(),
        };
        contents
            .get_mut(path)
            .unwrap()
            .extend_from_slice(line.as_bytes());
    }
    Ok(contents)
}

fn pad_source_bytes(
    manifest: &WorkloadManifest,
    paths: &[PathBuf],
    contents: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), String> {
    let current = contents.values().map(Vec::len).sum::<usize>();
    if current > manifest.source_bytes {
        return Err(format!(
            "generated symbols require {current} bytes, exceeding manifest source_bytes {}",
            manifest.source_bytes
        ));
    }
    let target = paths
        .last()
        .ok_or_else(|| "workload has no source files".to_string())?;
    contents
        .get_mut(target)
        .unwrap()
        .extend(std::iter::repeat_n(b' ', manifest.source_bytes - current));
    Ok(())
}

fn write_config(root: &Path) -> Result<(), String> {
    fs::write(
        root.join("criv.toml"),
        r#"[vault]
docs = "docs"
adr = "adr"

[source]
roots = ["src"]
exclude = []

[index]
source = true
embeddings = false

[state]
keep = 20

[enforce]
stages = ["commit", "push", "ci"]
"#,
    )
    .map_err(display_error)?;
    fs::write(
        root.join(".rumdl.toml"),
        "[global]\ndisable = [\"MD013\", \"MD025\", \"MD041\", \"MD075\"]\n",
    )
    .map_err(display_error)
}

fn write_notes(
    root: &Path,
    manifest: &WorkloadManifest,
    source_paths: &[PathBuf],
) -> Result<(), String> {
    let decision_ids = (1..=manifest.decisions)
        .map(|index| format!("{index:04}-decision-{index:04}|ADR-{index:04}"))
        .collect::<Vec<_>>();
    let doc_ids = (1..=manifest.docs)
        .map(|index| format!("document-{index:04}|doc-{index:04}"))
        .collect::<Vec<_>>();
    let ids = decision_ids
        .iter()
        .chain(&doc_ids)
        .cloned()
        .collect::<Vec<_>>();
    let mut links = vec![Vec::<String>::new(); manifest.notes];
    for index in 0..manifest.note_links {
        let owner = index % manifest.notes;
        let target = &ids[(index + owner + 1) % ids.len()];
        links[owner].push(target.clone());
    }

    for index in 1..=manifest.decisions {
        let id = format!("ADR-{index:04}");
        let mut frontmatter = format!(
            "---\nid: {id}\nkind: decision\ntitle: Decision {index:04}\nstatus: accepted\n"
        );
        if index <= manifest.policies {
            frontmatter.push_str(&format!(
                "governs:\n  - src/**/*.rs\npolicy:\n  patterns:\n    - id: policy-{index:04}\n      language: rust\n      pattern: \"println!($$$ARGS)\"\n      message: Generated performance policy {index:04}.\n"
            ));
        }
        frontmatter.push_str("---\n\n");
        let body = note_body(&format!("Decision {index:04}"), &links[index - 1]);
        fs::write(
            root.join("docs/adr")
                .join(format!("{index:04}-decision-{index:04}.md")),
            format!("{frontmatter}{body}"),
        )
        .map_err(display_error)?;
    }

    for index in 1..=manifest.docs {
        let owner = manifest.decisions + index - 1;
        let mut frontmatter =
            format!("---\nid: doc-{index:04}\nkind: doc\ntitle: Document {index:04}\n");
        if index == 1 && manifest.source_references > 0 {
            frontmatter.push_str("targets:\n  symbols:\n");
            for path in source_paths.iter().take(manifest.source_references) {
                frontmatter.push_str(&format!("    - {}\n", path.display()));
            }
        }
        frontmatter.push_str("---\n\n");
        let body = note_body(&format!("Document {index:04}"), &links[owner]);
        fs::write(
            root.join("docs").join(format!("document-{index:04}.md")),
            format!("{frontmatter}{body}"),
        )
        .map_err(display_error)?;
    }
    Ok(())
}

fn note_body(title: &str, links: &[String]) -> String {
    let mut body = format!("# {title}\n\nDeterministic generated workload content.\n");
    for target in links {
        body.push_str(&format!("\nSee [[{target}]].\n"));
    }
    body
}

fn write_c4_artifacts(root: &Path, count: usize) -> Result<(), String> {
    for index in 0..count {
        let specification = if index == 0 {
            "specification {\n  element person\n  element softwareSystem\n}\n\n"
        } else {
            ""
        };
        fs::write(
            root.join("docs/architecture")
                .join(format!("generated-context-{index:04}.c4")),
            format!(
                "{specification}model {{\n  person_{index} = person 'Person {index}' {{\n    description 'Uses the generated system'\n  }}\n  system_{index} = softwareSystem 'System {index}' {{\n    description 'Generated system'\n  }}\n  person_{index} -> system_{index} 'Uses'\n}}\n"
            ),
        )
        .map_err(display_error)?;
    }
    Ok(())
}

fn initialize_git(root: &Path) -> Result<(), String> {
    run_git(root, &["init", "-q", "-b", "main"])?;
    run_git(root, &["config", "user.email", "performance@criv.invalid"])?;
    run_git(root, &["config", "user.name", "criv performance"])?;
    run_git(
        root,
        &[
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "generated workload baseline",
        ],
    )?;
    run_git(root, &["add", "."])?;
    run_git(root, &["commit", "-q", "-m", "generated workload"])
}

fn run_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

pub fn tree_digest(root: &Path) -> Result<String, String> {
    let mut paths = Vec::new();
    collect_files(root, root, &mut paths)?;
    paths.sort();
    let mut hasher = blake3::Hasher::new();
    for relative in paths {
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        hasher.update(&fs::read(root.join(&relative)).map_err(display_error)?);
        hasher.update(&[0]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn collect_files(root: &Path, current: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(display_error)? {
        let entry = entry.map_err(display_error)?;
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(display_error)?;
        let first = relative
            .components()
            .next()
            .and_then(|part| part.as_os_str().to_str());
        if matches!(first, Some(".git" | ".criv")) {
            continue;
        }
        let file_type = entry.file_type().map_err(display_error)?;
        if file_type.is_dir() {
            collect_files(root, &path, paths)?;
        } else if file_type.is_file() {
            paths.push(relative.to_path_buf());
        } else {
            return Err(format!(
                "generated workload contains unsupported path {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::LoadedManifest;

    #[test]
    fn generation_is_deterministic_and_matches_manifest_shape() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in ["barrs-small.toml", "criv-medium.toml"] {
            let loaded =
                LoadedManifest::load(&repository.join("fixtures/performance").join(name)).unwrap();
            let first = tempfile::TempDir::new().unwrap();
            let second = tempfile::TempDir::new().unwrap();
            let first_generated = generate(first.path(), &loaded.manifest).unwrap();
            let second_generated = generate(second.path(), &loaded.manifest).unwrap();
            assert_eq!(first_generated.digest, second_generated.digest);
            assert_eq!(
                first_generated.source_paths.len(),
                loaded.manifest.source_files
            );
            let bytes = first_generated
                .source_paths
                .iter()
                .map(|path| fs::metadata(first.path().join(path)).unwrap().len() as usize)
                .sum::<usize>();
            assert_eq!(bytes, loaded.manifest.source_bytes);
            assert_eq!(
                fs::read_dir(first.path().join("docs/adr")).unwrap().count(),
                loaded.manifest.decisions
            );
            assert!(
                Command::new("git")
                    .args(["rev-parse", "--verify", "HEAD^"])
                    .current_dir(first.path())
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "generated workloads need an explicit CI comparison base"
            );
            if loaded.manifest.c4_artifacts > 0 {
                let c4 = fs::read_to_string(
                    first
                        .path()
                        .join("docs/architecture/generated-context-0000.c4"),
                )
                .unwrap();
                assert!(c4.starts_with("specification {\n"));
                assert!(c4.contains("model {\n"));
                assert!(c4.contains("Uses the generated system"));
            }
        }
    }

    #[test]
    fn mutation_preserves_file_size_and_touches_the_declared_count() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let loaded =
            LoadedManifest::load(&repository.join("fixtures/performance/barrs-small.toml"))
                .unwrap();
        let root = tempfile::TempDir::new().unwrap();
        let generated = generate(root.path(), &loaded.manifest).unwrap();
        let before = generated
            .source_paths
            .iter()
            .map(|path| fs::read(root.path().join(path)).unwrap())
            .collect::<Vec<_>>();
        mutate_sources(
            root.path(),
            &generated,
            loaded.manifest.changed_source_files,
        )
        .unwrap();
        let changed = generated
            .source_paths
            .iter()
            .zip(before)
            .filter(|(path, before)| fs::read(root.path().join(path)).unwrap() != *before)
            .count();
        assert_eq!(changed, loaded.manifest.changed_source_files);
    }

    #[test]
    fn mutation_prefers_supported_code_in_mixed_workloads() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let loaded =
            LoadedManifest::load(&repository.join("fixtures/performance/criv-medium.toml"))
                .unwrap();
        let root = tempfile::TempDir::new().unwrap();
        let generated = generate(root.path(), &loaded.manifest).unwrap();
        let before = generated
            .source_paths
            .iter()
            .map(|path| (path, fs::read(root.path().join(path)).unwrap()))
            .collect::<Vec<_>>();

        mutate_sources(root.path(), &generated, 1).unwrap();

        let changed_path = before
            .into_iter()
            .find_map(|(path, before)| {
                (fs::read(root.path().join(path)).unwrap() != before).then_some(path)
            })
            .unwrap();
        assert_eq!(
            changed_path.extension().and_then(|value| value.to_str()),
            Some("rs")
        );
    }
}

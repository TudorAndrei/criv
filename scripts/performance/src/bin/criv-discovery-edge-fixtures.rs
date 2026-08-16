use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, ValueEnum};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-edge-fixtures",
    about = "Generate one deterministic file-discovery edge-case repository"
)]
struct Args {
    /// Edge case to generate.
    #[arg(long, value_enum)]
    case: EdgeCase,
    /// New generated repository directory. It must not exist.
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum EdgeCase {
    MatchedParity,
    SourceShape,
    SourceLink,
    VaultShape,
    VaultLink,
    MarkdownShape,
    MarkdownInvalidPattern,
    MissingRoots,
    NonUtf8Source,
}

impl EdgeCase {
    fn id(self) -> &'static str {
        match self {
            Self::MatchedParity => "matched-parity",
            Self::SourceShape => "source-shape",
            Self::SourceLink => "source-link",
            Self::VaultShape => "vault-shape",
            Self::VaultLink => "vault-link",
            Self::MarkdownShape => "markdown-shape",
            Self::MarkdownInvalidPattern => "markdown-invalid-pattern",
            Self::MissingRoots => "missing-roots",
            Self::NonUtf8Source => "non-utf8-source",
        }
    }
}

#[derive(Debug, Serialize)]
struct TargetExpectation {
    outcome: &'static str,
    paths: BTreeMap<&'static str, Vec<String>>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    id: &'static str,
    case: EdgeCase,
    target: BTreeMap<&'static str, TargetExpectation>,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-edge-fixtures: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
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
    if let Err(error) = generate(args.case, &args.output) {
        cleanup_output(&args.output);
        return Err(error);
    }
    let receipt = Receipt {
        schema: "criv.discovery-edge-fixture.v1",
        id: args.case.id(),
        case: args.case,
        target: target_expectations(args.case),
    };
    write_json(args.output.join("discovery-edge-fixture.json"), &receipt)?;
    if let Err(error) = initialize_git(&args.output) {
        cleanup_output(&args.output);
        return Err(error);
    }
    println!("{}", args.output.display());
    Ok(())
}

fn generate(case: EdgeCase, root: &Path) -> Result<(), String> {
    write_file(
        root,
        "criv.toml",
        b"[vault]\ndocs = \"docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"src\"]\nexclude = [\"**/excluded/**\"]\n\n[index]\nsource = true\n",
    )?;
    write_file(
        root,
        ".rumdl.toml",
        b"[global]\nexclude = [\".criv/**\"]\nrespect_gitignore = true\n",
    )?;
    write_file(root, ".gitignore", b".criv/\n")?;
    fs::create_dir(root.join("src")).map_err(display_error)?;
    fs::create_dir(root.join("docs")).map_err(display_error)?;

    match case {
        EdgeCase::MatchedParity => {
            write_file(root, "src/a.rs", b"pub fn a() {}\n")?;
            write_note(root, "docs/a.md", "a")?;
            write_file(root, "docs/model.c4", b"model {}\n")?;
            write_file(root, "guide.md", b"# Guide\n")?;
        }
        EdgeCase::SourceShape => {
            write_file(
                root,
                "criv.toml",
                b"[vault]\ndocs = \"docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"src\", \"src/nested\", \"explicit.txt\", \"missing\"]\nexclude = [\"**/excluded/**\"]\n\n[index]\nsource = true\n",
            )?;
            write_file(root, ".gitignore", b".criv/\nsrc/ignored.rs\n")?;
            write_file(root, "src/a.rs", b"pub fn a() {}\n")?;
            write_file(root, "src/nested/b.rs", b"pub fn b() {}\n")?;
            write_file(root, "src/.hidden.rs", b"pub fn hidden() {}\n")?;
            write_file(root, "src/ignored.rs", b"pub fn ignored() {}\n")?;
            write_file(root, "src/excluded/c.rs", b"pub fn excluded() {}\n")?;
            write_file(root, "src/binary.bin", &[0, 1, 2, 3, 0, 255])?;
            write_file(root, "explicit.txt", b"explicit source root\n")?;
        }
        EdgeCase::SourceLink => {
            write_file(root, "src/a.rs", b"pub fn a() {}\n")?;
            symlink_file(Path::new("a.rs"), &root.join("src/link.rs"))?;
        }
        EdgeCase::VaultShape => {
            write_note(root, "docs/a.md", "a")?;
            write_note(root, "docs/.hidden.md", "hidden")?;
            write_file(root, "docs/model.c4", b"model {}\n")?;
            write_note(root, "docs/UPPER.MD", "upper")?;
            write_note(root, "docs/other.markdown", "other")?;
            for path in [
                "docs/target/skip.md",
                "docs/node_modules/skip.md",
                "docs/.criv/skip.md",
                "docs/.git/skip.md",
            ] {
                write_note(root, path, "skip")?;
            }
        }
        EdgeCase::VaultLink => {
            write_note(root, "docs/a.md", "a")?;
            symlink_file(Path::new("a.md"), &root.join("docs/link.md"))?;
        }
        EdgeCase::MarkdownShape => {
            write_file(
                root,
                ".rumdl.toml",
                b"[global]\ninclude = [\"content/**\", \".hidden.md\"]\nexclude = [\"content/excluded/**\"]\nrespect_gitignore = true\n",
            )?;
            write_file(root, ".gitignore", b".criv/\ncontent/ignored.md\n")?;
            write_file(root, ".hidden.md", b"# Hidden\n")?;
            write_file(root, "content/a.md", b"# A\n")?;
            write_file(root, "content/b.markdown", b"# B\n")?;
            write_file(root, "content/UPPER.MD", b"# Upper\n")?;
            write_file(root, "content/ignored.md", b"# Ignored\n")?;
            write_file(root, "content/excluded/c.md", b"# Excluded\n")?;
            write_file(root, "content/not-markdown.txt", b"not Markdown\n")?;
        }
        EdgeCase::MarkdownInvalidPattern => {
            write_file(
                root,
                ".rumdl.toml",
                b"[global]\ninclude = [\"[\"]\nrespect_gitignore = true\n",
            )?;
            write_file(root, "a.md", b"# A\n")?;
        }
        EdgeCase::MissingRoots => {
            write_file(
                root,
                "criv.toml",
                b"[vault]\ndocs = \"missing-docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"missing-source\"]\nexclude = []\n\n[index]\nsource = true\n",
            )?;
        }
        EdgeCase::NonUtf8Source => write_non_utf8_source(root)?,
    }
    Ok(())
}

fn target_expectations(case: EdgeCase) -> BTreeMap<&'static str, TargetExpectation> {
    let mut expected = BTreeMap::new();
    match case {
        EdgeCase::MatchedParity => {
            success(&mut expected, "source", "source", &["src/a.rs"]);
            success_groups(
                &mut expected,
                "vault",
                &[("markdown", &["docs/a.md"]), ("c4", &["docs/model.c4"])],
            );
            success(
                &mut expected,
                "markdown",
                "markdown",
                &["docs/a.md", "guide.md"],
            );
        }
        EdgeCase::SourceShape => success(
            &mut expected,
            "source",
            "source",
            &[
                "explicit.txt",
                "src/.hidden.rs",
                "src/a.rs",
                "src/ignored.rs",
                "src/nested/b.rs",
            ],
        ),
        EdgeCase::SourceLink => error(&mut expected, "source"),
        EdgeCase::VaultShape => success_groups(
            &mut expected,
            "vault",
            &[
                ("markdown", &["docs/.hidden.md", "docs/a.md"]),
                ("c4", &["docs/model.c4"]),
            ],
        ),
        EdgeCase::VaultLink => error(&mut expected, "vault"),
        EdgeCase::MarkdownShape => success(
            &mut expected,
            "markdown",
            "markdown",
            &[
                ".hidden.md",
                "content/UPPER.MD",
                "content/a.md",
                "content/b.markdown",
            ],
        ),
        EdgeCase::MarkdownInvalidPattern => error(&mut expected, "markdown"),
        EdgeCase::MissingRoots => {
            success(&mut expected, "source", "source", &[]);
            success_groups(&mut expected, "vault", &[("markdown", &[]), ("c4", &[])]);
        }
        EdgeCase::NonUtf8Source => error(&mut expected, "source"),
    }
    expected
}

fn success(
    expected: &mut BTreeMap<&'static str, TargetExpectation>,
    profile: &'static str,
    group: &'static str,
    paths: &[&str],
) {
    success_groups(expected, profile, &[(group, paths)]);
}

fn success_groups(
    expected: &mut BTreeMap<&'static str, TargetExpectation>,
    profile: &'static str,
    groups: &[(&'static str, &[&str])],
) {
    expected.insert(
        profile,
        TargetExpectation {
            outcome: "success",
            paths: groups
                .iter()
                .map(|(group, paths)| {
                    (
                        *group,
                        paths.iter().map(|path| (*path).to_string()).collect(),
                    )
                })
                .collect(),
        },
    );
}

fn error(expected: &mut BTreeMap<&'static str, TargetExpectation>, profile: &'static str) {
    expected.insert(
        profile,
        TargetExpectation {
            outcome: "error",
            paths: BTreeMap::new(),
        },
    );
}

fn write_note(root: &Path, path: &str, id: &str) -> Result<(), String> {
    write_file(
        root,
        path,
        format!("---\nid: {id}\nkind: doc\ntitle: {id}\n---\n\n# {id}\n").as_bytes(),
    )
}

fn write_file(root: &Path, relative: &str, contents: &[u8]) -> Result<(), String> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(display_error)?;
    }
    fs::write(path, contents).map_err(display_error)
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(display_error)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_file(target, link).map_err(display_error)
}

#[cfg(unix)]
fn write_non_utf8_source(root: &Path) -> Result<(), String> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut name = b"non-utf8-".to_vec();
    name.push(0xff);
    name.extend_from_slice(b".rs");
    fs::write(
        root.join("src").join(OsString::from_vec(name)),
        b"pub fn non_utf8() {}\n",
    )
    .map_err(display_error)
}

#[cfg(windows)]
fn write_non_utf8_source(_root: &Path) -> Result<(), String> {
    Err("the non-UTF-8 source fixture is not representable on Windows".into())
}

fn initialize_git(root: &Path) -> Result<(), String> {
    for args in [
        &["init", "--quiet"][..],
        &["config", "user.email", "performance@criv.invalid"][..],
        &["config", "user.name", "criv performance"][..],
        &["add", "--all"][..],
        &["commit", "--quiet", "-m", "edge fixture root"][..],
    ] {
        let output = Command::new("git")
            .args(args)
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

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(display_error)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(display_error)
}

fn cleanup_output(output: &Path) {
    if let Err(error) = fs::remove_dir_all(output) {
        eprintln!(
            "criv-discovery-edge-fixtures: failed to remove incomplete output {}: {error}",
            output.display()
        );
    }
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_shape_receipt_has_duplicate_free_target_paths() {
        let expected = target_expectations(EdgeCase::SourceShape);
        assert_eq!(expected["source"].paths["source"].len(), 5);
    }

    #[test]
    fn matched_fixture_creates_all_three_profile_inputs() {
        let root = tempfile::TempDir::new().unwrap();
        generate(EdgeCase::MatchedParity, root.path()).unwrap();
        assert!(root.path().join("src/a.rs").is_file());
        assert!(root.path().join("docs/a.md").is_file());
        assert!(root.path().join("docs/model.c4").is_file());
        assert!(root.path().join("guide.md").is_file());
    }
}

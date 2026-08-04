use std::path::Path;

use crate::Result;
use crate::c4_code;
use crate::config::ArchitectureCodeConfig;
use crate::util::write_atomic_if_changed_in;
use crate::vault::Vault;

pub(crate) fn write_code_architecture(root: &Path, vault: &Vault) -> Result<bool> {
    let Some(config) = &vault.config.architecture_code else {
        return Ok(false);
    };

    write_code_architecture_with_config(root, vault, config)
}

fn write_code_architecture_with_config(
    root: &Path,
    vault: &Vault,
    config: &ArchitectureCodeConfig,
) -> Result<bool> {
    let content = code_architecture_content(vault, config);
    write_atomic_if_changed_in(
        root,
        Path::new(&vault.config.docs_dir),
        Path::new(&config.output),
        &content,
    )
}

fn code_architecture_content(vault: &Vault, config: &ArchitectureCodeConfig) -> String {
    let _ = &config.title;
    code_architecture_artifact(vault)
}

fn code_architecture_artifact(vault: &Vault) -> String {
    format!(
        "{}\n",
        c4_code::for_all_indexed_sources_likec4(vault).join("\n")
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::util::read_to_string;

    #[test]
    fn code_architecture_write_reports_changed_then_noop() {
        let temp = TempDir::new().unwrap();
        write_architecture_fixture(temp.path(), true);
        let vault = Vault::load(temp.path()).unwrap();

        assert!(write_code_architecture(temp.path(), &vault).unwrap());
        assert!(!write_code_architecture(temp.path(), &vault).unwrap());

        let output = read_to_string(&temp.path().join("docs/architecture/04-code.c4")).unwrap();
        assert!(output.starts_with("// criv:generated true\nspecification"));
        assert!(output.contains("element module"));
        assert!(output.contains("link ../../src/lib.rs 'source'"));
        assert!(!output.contains("fn run"));
    }

    #[test]
    fn code_architecture_uses_no_indexed_source_message() {
        let temp = TempDir::new().unwrap();
        write_architecture_fixture(temp.path(), false);
        let vault = Vault::load(temp.path()).unwrap();

        assert!(write_code_architecture(temp.path(), &vault).unwrap());

        let output = read_to_string(&temp.path().join("docs/architecture/04-code.c4")).unwrap();
        assert!(output.contains("noModules = module 'No indexed modules'"));
    }

    #[test]
    fn code_architecture_c4_output_writes_likec4_modules() {
        let temp = TempDir::new().unwrap();
        write_architecture_fixture_with_output(temp.path(), true, "docs/architecture/04-code.c4");
        let vault = Vault::load(temp.path()).unwrap();

        assert!(write_code_architecture(temp.path(), &vault).unwrap());
        assert!(!write_code_architecture(temp.path(), &vault).unwrap());

        let output = read_to_string(&temp.path().join("docs/architecture/04-code.c4")).unwrap();
        assert!(output.starts_with("// criv:generated true\nspecification"));
        assert!(output.contains("element module"));
        assert!(!output.contains("digraph"));
        assert!(!output.contains("classDiagram"));
    }

    #[cfg(unix)]
    #[test]
    fn code_architecture_rejects_symlinked_output_parent() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        write_architecture_fixture_with_output(temp.path(), true, "docs/architecture/04-code.c4");
        symlink(outside.path(), temp.path().join("docs/architecture")).unwrap();
        let vault = Vault::load(temp.path()).unwrap();

        let error = write_code_architecture(temp.path(), &vault).unwrap_err();

        assert!(error.to_string().contains("symlinked vault path component"));
        assert!(!outside.path().join("04-code.c4").exists());
    }

    fn write_architecture_fixture(root: &Path, source_index: bool) {
        write_architecture_fixture_with_output(root, source_index, "docs/architecture/04-code.c4");
    }

    fn write_architecture_fixture_with_output(root: &Path, source_index: bool, output: &str) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("criv.toml"),
            format!(
                r#"
[source]
roots = ["src"]

[index]
source = {source_index}

[architecture.code]
output = "{output}"
title = "Code diagram for criv"
"#
            ),
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            r#"
fn run() {
    helper();
}

fn helper() {}
"#,
        )
        .unwrap();
    }
}

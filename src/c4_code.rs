use std::collections::BTreeSet;

use crate::source_graph::SymbolKind;
use crate::vault::Vault;

pub(crate) fn for_glob(vault: &Vault, glob: &str) -> Vec<String> {
    let in_scope = vault
        .source_files_matching_glob(glob)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if in_scope.is_empty() {
        return vec![
            "classDiagram".into(),
            format!("%% no source files matched `{glob}`"),
        ];
    }

    render(vault, &in_scope)
}

#[allow(dead_code)]
pub(crate) fn for_all_indexed_sources(vault: &Vault) -> Vec<String> {
    let in_scope = vault
        .source_files()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if in_scope.is_empty() {
        return vec![
            "classDiagram".into(),
            "%% no indexed source files available".into(),
        ];
    }

    render(vault, &in_scope)
}

fn render(vault: &Vault, in_scope: &BTreeSet<String>) -> Vec<String> {
    let mut classes = BTreeSet::new();
    let mut edges = BTreeSet::new();

    for symbol in vault.source_graph().symbols() {
        if !in_scope.contains(&symbol.id.path) || !is_c4_code_symbol(symbol.kind) {
            continue;
        }
        classes.insert(symbol.name.clone());
        for call in &symbol.calls {
            let Some(target) = vault.source_graph().resolve_call(&symbol.id, &call.target) else {
                continue;
            };
            if in_scope.contains(&target.path) {
                edges.insert(format!("{} --> {}", symbol.name, target.name));
            }
        }
    }

    let mut rows = vec!["classDiagram".into()];
    rows.extend(classes.into_iter().map(|name| format!("class {name}")));
    rows.extend(edges);
    rows
}

fn is_c4_code_symbol(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Function | SymbolKind::Method
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn scoped_code_diagram_emits_in_scope_classes_and_edges() {
        let temp = TempDir::new().unwrap();
        write_c4_code_fixture(temp.path());
        let vault = Vault::load(temp.path()).unwrap();

        let rows = for_glob(&vault, "src/lib.rs");

        assert!(rows.contains(&"classDiagram".to_string()));
        assert!(rows.contains(&"class Foo".to_string()));
        assert!(rows.contains(&"class run".to_string()));
        assert!(rows.contains(&"class helper".to_string()));
        assert!(rows.contains(&"run --> helper".to_string()));
        assert!(!rows.contains(&"class external".to_string()));
        assert!(!rows.contains(&"run --> external".to_string()));
    }

    #[test]
    fn scoped_code_diagram_reports_empty_source_glob_as_valid_mermaid() {
        let temp = TempDir::new().unwrap();
        write_c4_code_fixture(temp.path());
        let vault = Vault::load(temp.path()).unwrap();

        let rows = for_glob(&vault, "src/missing.rs");

        assert_eq!(
            rows,
            vec![
                "classDiagram".to_string(),
                "%% no source files matched `src/missing.rs`".to_string(),
            ]
        );
    }

    #[test]
    fn all_indexed_source_diagram_includes_cross_file_edges() {
        let temp = TempDir::new().unwrap();
        write_c4_code_fixture(temp.path());
        let vault = Vault::load(temp.path()).unwrap();

        let rows = for_all_indexed_sources(&vault);

        assert!(rows.contains(&"class external".to_string()));
        assert!(rows.contains(&"run --> external".to_string()));
    }

    fn write_c4_code_fixture(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("other")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src", "other"]
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            r#"
struct Foo;

impl Foo {
    fn run(&self) {
        helper();
        external();
    }
}

fn helper() {}
"#,
        )
        .unwrap();
        fs::write(root.join("other/out.rs"), "fn external() {}\n").unwrap();
    }
}

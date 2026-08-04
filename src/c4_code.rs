use std::collections::{BTreeMap, BTreeSet};

use crate::source_graph::{Language, SourceFile};
use crate::vault::Vault;

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct ModuleNode {
    id: String,
    name: String,
    path: String,
    language: Language,
}

pub(crate) fn for_glob(vault: &Vault, glob: &str) -> Vec<String> {
    let in_scope = vault
        .source_files_matching_glob(glob)
        .into_iter()
        .collect::<BTreeSet<_>>();
    render(vault, &in_scope)
}

pub(crate) fn for_all_indexed_sources_likec4(vault: &Vault) -> Vec<String> {
    render(vault, &vault.source_files().iter().cloned().collect())
}

fn render(vault: &Vault, in_scope: &BTreeSet<String>) -> Vec<String> {
    let files = vault
        .source_graph()
        .files
        .values()
        .filter(|file| in_scope.contains(&file.path))
        .collect::<Vec<_>>();
    let mut nodes = files
        .iter()
        .flat_map(|file| modules_for_file(vault, file))
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        (&left.name, left.language, &left.path).cmp(&(&right.name, right.language, &right.path))
    });
    nodes.dedup_by(|left, right| left.name == right.name && left.language == right.language);

    let primary = files
        .iter()
        .map(|file| (file.path.as_str(), module_name(vault, file)))
        .collect::<BTreeMap<_, _>>();
    let mut edges = BTreeSet::new();
    for file in &files {
        let Some(source_name) = primary.get(file.path.as_str()) else {
            continue;
        };
        let Some(source) = nodes.iter().find(|node| node.name == *source_name) else {
            continue;
        };
        for import in &file.imports {
            if let Some(target) = resolve_import(vault, &nodes, file, &import.module)
                && source.id != target.id
            {
                edges.insert((source.id.clone(), target.id.clone()));
            }
        }
    }

    let mut rows = vec![
        "// criv:generated true".into(),
        "specification {".into(),
        "  element module".into(),
        "}".into(),
        String::new(),
        "model {".into(),
    ];
    if nodes.is_empty() {
        rows.push("  noModules = module 'No indexed modules'".into());
    } else {
        for node in &nodes {
            rows.push(format!(
                "  {} = module '{}' {{",
                node.id,
                dsl_string(&node.name)
            ));
            rows.push(format!("    technology '{}'", language_name(node.language)));
            rows.push(format!("    link ../../{} 'source'", node.path));
            rows.push("  }".into());
        }
        for (source, target) in edges {
            rows.push(format!("  {source} -> {target} 'imports'"));
        }
    }
    rows.extend(["}".into(), String::new(), "views {".into()]);
    if nodes.is_empty() {
        rows.extend([
            "  view codeModules {".into(),
            "    title 'Code modules'".into(),
            "    include noModules".into(),
            "  }".into(),
        ]);
    } else {
        for language in [
            Language::Rust,
            Language::TypeScript,
            Language::JavaScript,
            Language::Python,
            Language::Go,
        ] {
            let language_nodes = nodes
                .iter()
                .filter(|node| node.language == language)
                .collect::<Vec<_>>();
            if language_nodes.is_empty() {
                continue;
            }
            rows.push(format!("  view code{} {{", language_name(language)));
            rows.push(format!("    title '{} modules'", language_name(language)));
            for node in language_nodes {
                rows.push(format!("    include {}", node.id));
            }
            rows.push("    include * -> *".into());
            rows.push("  }".into());
        }
    }
    rows.push("}".into());
    rows
}

fn modules_for_file(vault: &Vault, file: &SourceFile) -> Vec<ModuleNode> {
    if file.language == Language::Unknown {
        return Vec::new();
    }
    let base = module_name(vault, file);
    let mut modules = vec![module_node(base.clone(), &file.path, file.language)];
    modules.extend(file.modules.iter().filter_map(|decl| {
        if file.language == Language::Go || decl.name == base {
            return None;
        }
        Some(module_node(
            format!("{base}::{}", decl.name),
            &format!("{}#L{}", file.path, decl.line),
            file.language,
        ))
    }));
    modules
}

fn module_node(name: String, path: &str, language: Language) -> ModuleNode {
    ModuleNode {
        id: format!(
            "m_{:016x}",
            stable_id(&format!("{name}\0{}", language_name(language)))
        ),
        name,
        path: path.to_string(),
        language,
    }
}

fn module_name(vault: &Vault, file: &SourceFile) -> String {
    let path = file.path.replace('\\', "/");
    let without_extension = path
        .rsplit_once('.')
        .map_or(path.as_str(), |(base, _)| base);
    match file.language {
        Language::Rust => rust_module_name(vault, &path).unwrap_or_else(|| {
            without_extension
                .strip_suffix("/lib")
                .or_else(|| without_extension.strip_suffix("/main"))
                .unwrap_or(without_extension)
                .replace('/', "::")
        }),
        Language::TypeScript | Language::JavaScript => without_extension
            .strip_suffix("/index")
            .unwrap_or(without_extension)
            .replace('/', "::"),
        Language::Python => {
            let relative = configured_source_relative(vault, without_extension);
            relative
                .strip_suffix("/__init__")
                .unwrap_or(relative)
                .replace('/', "::")
        }
        Language::Go => {
            let package = file.modules.first().map(|module| module.name.as_str());
            let directory = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("root");
            format!(
                "{}::{}",
                directory.replace('/', "::"),
                package.unwrap_or("main")
            )
        }
        Language::Unknown => path,
    }
}

fn resolve_import<'a>(
    vault: &Vault,
    nodes: &'a [ModuleNode],
    source: &SourceFile,
    import: &str,
) -> Option<&'a ModuleNode> {
    let normalized = import
        .trim_start_matches("crate::")
        .trim_start_matches("./")
        .trim_start_matches("../")
        .replace(['/', '.'], "::");
    let source_name = module_name(vault, source);
    let source_parent = source_name.rsplit_once("::").map(|(parent, _)| parent);
    nodes.iter().find(|node| {
        node.name == normalized
            || node.name.ends_with(&format!("::{normalized}"))
            || source_parent.is_some_and(|parent| node.name == format!("{parent}::{normalized}"))
            || normalized
                .rsplit("::")
                .next()
                .is_some_and(|tail| node.name.ends_with(&format!("::{tail}")))
    })
}

fn configured_source_relative<'a>(vault: &'a Vault, path: &'a str) -> &'a str {
    vault
        .config
        .source_roots
        .iter()
        .filter(|root| root.as_str() != ".")
        .filter_map(|root| {
            path.strip_prefix(root)
                .and_then(|rest| {
                    rest.strip_prefix('/')
                        .or_else(|| rest.is_empty().then_some(rest))
                })
                .map(|relative| (root.len(), relative))
        })
        .max_by_key(|(length, _)| *length)
        .map_or(path, |(_, relative)| relative)
}

fn rust_module_name(vault: &Vault, path: &str) -> Option<String> {
    let source = std::path::Path::new(path);
    let mut directory = source.parent();
    while let Some(candidate_dir) = directory {
        let manifest = vault.root.join(candidate_dir).join("Cargo.toml");
        if manifest.is_file()
            && let Ok(contents) = std::fs::read_to_string(&manifest)
            && let Ok(value) = toml::from_str::<toml::Value>(&contents)
            && let Some(package) = value
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml::Value::as_str)
        {
            let source_dir = candidate_dir.join("src");
            if let Ok(relative) = source.strip_prefix(&source_dir) {
                return Some(rust_module_from_relative(package, relative));
            }
        }
        directory = candidate_dir.parent();
    }
    None
}

fn rust_module_from_relative(package: &str, relative: &std::path::Path) -> String {
    let mut parts = relative
        .iter()
        .map(|part| part.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if let Some(last) = parts.last_mut()
        && let Some((stem, _)) = last.rsplit_once('.')
    {
        *last = stem.to_string();
    }
    let crate_name = package.replace('-', "_");
    if parts.first().is_some_and(|part| part == "bin") && parts.len() > 1 {
        let binary = parts[1].clone();
        parts.drain(0..2);
        if parts.last().is_some_and(|part| part == "main") {
            parts.pop();
        }
        parts.insert(0, binary);
    } else {
        if parts
            .last()
            .is_some_and(|part| matches!(part.as_str(), "lib" | "main" | "mod"))
        {
            parts.pop();
        }
        parts.insert(0, crate_name);
    }
    parts.join("::")
}

fn language_name(language: Language) -> &'static str {
    match language {
        Language::Rust => "Rust",
        Language::TypeScript => "TypeScript",
        Language::JavaScript => "JavaScript",
        Language::Python => "Python",
        Language::Go => "Go",
        Language::Unknown => "Unknown",
    }
}

fn dsl_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn stable_id(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn generated_code_uses_modules_and_imports_only() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::create_dir_all(temp.path().join("docs")).unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"demo-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("criv.toml"),
            "[source]\nroots = [\"src\"]\n[index]\nsource = true\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "mod api;\nuse crate::api;\nfn run() {}\n",
        )
        .unwrap();
        fs::write(temp.path().join("src/api.rs"), "pub fn call() {}\n").unwrap();
        let vault = Vault::load(temp.path()).unwrap();
        assert!(vault.root.join("Cargo.toml").is_file(), "{:?}", vault.root);
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(vault.root.join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(manifest["package"]["name"].as_str(), Some("demo-crate"));
        assert_eq!(
            rust_module_name(&vault, "src/lib.rs"),
            Some("demo_crate".into())
        );

        let output = for_all_indexed_sources_likec4(&vault).join("\n");

        assert!(output.contains("element module"));
        assert!(output.contains("demo_crate::api"), "{output}");
        assert!(output.contains("'imports'"));
        assert!(!output.contains("fn run"));
        assert!(!output.contains("classDiagram"));
        assert!(!output.contains("digraph"));
    }

    #[test]
    fn generated_code_uses_native_module_identities_and_nesting() {
        let temp = TempDir::new().unwrap();
        for directory in ["src", "py/pkg", "go", "docs"] {
            fs::create_dir_all(temp.path().join(directory)).unwrap();
        }
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"demo-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("criv.toml"),
            "[source]\nroots = [\"src\", \"py\", \"go\"]\n[index]\nsource = true\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "mod api;\nmod outer { mod inner {} }\n",
        )
        .unwrap();
        fs::write(temp.path().join("src/api.rs"), "pub struct Api;\n").unwrap();
        fs::write(
            temp.path().join("py/pkg/__init__.py"),
            "from . import tool\n",
        )
        .unwrap();
        fs::write(temp.path().join("py/pkg/tool.py"), "VALUE = 1\n").unwrap();
        fs::write(temp.path().join("go/a.go"), "package service\n").unwrap();
        fs::write(temp.path().join("go/b.go"), "package service\n").unwrap();

        let vault = Vault::load(temp.path()).unwrap();
        let output = for_all_indexed_sources_likec4(&vault).join("\n");

        assert!(output.contains("module 'demo_crate'"), "{output}");
        assert!(output.contains("module 'demo_crate::api'"));
        assert!(output.contains("module 'demo_crate::outer::inner'"));
        assert!(output.contains("module 'pkg'"));
        assert!(output.contains("module 'pkg::tool'"));
        assert_eq!(output.matches("module 'go::service'").count(), 1);
    }
}

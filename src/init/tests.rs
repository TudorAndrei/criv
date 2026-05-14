use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;

#[test]
fn init_writes_parseable_structured_templates() {
    let root = unique_temp_dir("criv-init-templates");
    run(
        &root,
        InitOptions {
            no_obsidian: false,
            no_skills: true,
        },
    )
    .unwrap();

    toml::from_str::<toml::Value>(&std::fs::read_to_string(root.join("criv.toml")).unwrap())
        .unwrap();
    serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(root.join(".criv/state.json")).unwrap(),
    )
    .unwrap();

    for path in [
        ".obsidian/plugins/criv/manifest.json",
        ".obsidian/plugins/criv/package.json",
        ".obsidian/plugins/criv/tsconfig.json",
        ".obsidian/plugins/criv/versions.json",
    ] {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(root.join(path)).unwrap(),
        )
        .unwrap();
    }

    let readme = std::fs::read_to_string(root.join("docs/adr/README.md")).unwrap();
    let frontmatter = readme
        .strip_prefix("---\n")
        .and_then(|value| value.split_once("---\n"))
        .map(|(frontmatter, _body)| frontmatter)
        .unwrap();
    serde_norway::from_str::<BTreeMap<String, serde_norway::Value>>(frontmatter).unwrap();

    let _ = std::fs::remove_dir_all(root);
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}
